#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Collect what one release run measured into metrics.json (release/metrics.schema.json): boot time,
# install time, installer pages, idle CPU, shell map time, ISO size and the journey pass counts.
# Why: every number on the site's Proof and Compare pages must trace to a file of a real run, with
# the command that made it; a number the run did not measure is null, never estimated (NS2).
# How: tests/metrics.sh RUN_DIR [OUT]   (OUT defaults to stdout; exit 1 when the result does not
# validate against the schema). RUN_DIR is a release run as wolos-build lays it out (/scratch/rel012):
#   iso/*.iso, iso/*.json          the image and its release record (size, sha256, kernel)
#   vm/<journey>/journey.jsonl     wolvm journey steps with seconds (tests/wolvm/wolvm journey)
#   vm/<journey>/serial.log, *-logs.txt, system.txt   console, journal and os-release of that VM
#   vm/<journey>/idle.txt          optional: design/tools/perf/idle.py output (idle CPU)
#   vm/*/vms/<vm>/vm.json          wolvm's VM configuration (CPUs, memory, GPU, TPM, Secure Boot)
# Needs python3 only (no network, no root, reads RUN_DIR and writes OUT).
set -euo pipefail
if [ $# -lt 1 ] || [ ! -d "$1" ]; then
  echo "usage: tests/metrics.sh RUN_DIR [OUT]" >&2
  exit 2
fi
here=$(cd "$(dirname "$0")/.." && pwd)
schema=$here/release/metrics.schema.json
run=$(cd "$1" && pwd)
out=${2:--}
python3 -I - "$run" "$schema" "$out" <<'PY'
import glob, json, os, re, sys, time

run, schema_path, out = sys.argv[1:4]
J = lambda *p: os.path.join(run, *p)
rel = lambda p: os.path.relpath(p, run)
evidence = {}


def read(p):
    try:
        with open(p, encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def journey(d):
    """Header and steps of vm/<d>/journey.jsonl."""
    head, steps = {}, []
    for line in read(J("vm", d, "journey.jsonl")).splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            o = json.loads(line)
        except ValueError:
            continue
        if "n" in o:
            steps.append(o)
        else:
            head = o
    return head, steps


def wolvm_cmd(d, head):
    f = head.get("file", "")
    k = f.find("tests/wolvm/journeys/")
    f = f[k:] if k >= 0 else f
    return f"python3 tests/wolvm/wolvm journey {head.get('vm', '?')} {f} --out RUN/vm/{d}"


dirs = sorted(os.path.basename(os.path.dirname(p)) for p in glob.glob(J("vm", "*", "journey.jsonl")))
journeys = {d: journey(d) for d in dirs}

# ---- journeys: pass counts ----
jl = []
for d in dirs:
    head, steps = journeys[d]
    bad = next((s for s in steps if not s.get("ok")), None)
    jl.append({
        "name": d,
        "title": head.get("journey", d),
        "steps": len(steps),
        "ok": sum(1 for s in steps if s.get("ok")),
        "passed": bool(steps) and bad is None,
        "failed_step": None if bad is None else f"{bad['n']} {bad.get('step', '')}".strip(),
    })

# ---- date: the first journey's start ----
date = None
for d in dirs:
    t = journeys[d][0].get("time")
    if t:
        try:
            date = time.strftime("%Y-%m-%d", time.strptime(t, "%a %b %d %H:%M:%S %Y"))
            break
        except ValueError:
            pass

# ---- the ISO ----
iso_json = sorted(glob.glob(J("iso", "*.json")))
rec = {}
if iso_json:
    try:
        rec = json.loads(read(iso_json[0]))
    except ValueError:
        rec = {}
isos = sorted(glob.glob(J("iso", "*.iso")))
iso_bytes = rec.get("size")
if iso_bytes is not None:
    evidence["iso_bytes"] = {"from": rel(iso_json[0]), "command": "wolos-build iso (build.sh writes the release record)"}
if isos:
    iso_bytes = os.path.getsize(isos[0])
    evidence["iso_bytes"] = {"from": rel(isos[0]), "command": "wolos-build iso (build.sh); stat -c %s on the image"}
if date is None and isos:
    date = time.strftime("%Y-%m-%d", time.gmtime(os.path.getmtime(isos[0])))

# ---- version and kernel: os-release of a booted VM, else the ISO record ----
version = rec.get("version")
kernel = rec.get("kernel")
for d in dirs:
    s = read(J("vm", d, "system.txt"))
    m = re.search(r"^VERSION_ID=\"?([^\"\n]+)", s, re.M)
    if m:
        version = m.group(1)
        k = re.search(r"^(\d+\.\d+\S*-wolos\S*)$", s, re.M)
        kernel = k.group(1) if k else kernel
        break

# ---- boot: VM start to the live image's first screen (journey live-boot, through the first wait-text) ----
boot_s = boot_systemd_s = None
if "live-boot" in journeys:
    head, steps = journeys["live-boot"]
    t = 0.0
    for s in steps:
        t += float(s.get("secs") or 0)
        if s.get("step", "").startswith("wait-text") and s.get("ok"):
            boot_s = round(t, 1)
            evidence["boot_s"] = {"from": "vm/live-boot/journey.jsonl", "command": wolvm_cmd("live-boot", head),
                                  "what": f"steps 1-{s['n']}: start to '{s['step'][10:]}'"}
            break
    for f in sorted(glob.glob(J("vm", "live-boot", "*logs.txt"))) + [J("vm", "live-boot", "serial.log")]:
        m = re.search(r"Startup finished in .*= ([0-9.]+)s", read(f))
        if m:
            boot_systemd_s = round(float(m.group(1)), 1)
            evidence["boot_systemd_s"] = {"from": rel(f), "command": "journey step 'logs' (journalctl -b) of " + wolvm_cmd("live-boot", head),
                                          "what": "systemd: Startup finished (kernel + userspace)"}
            break

# ---- install: 'wolos-install: prepare' to 'wolos-install: done' on the console ----
install_s = None
for d in [x for x in dirs if x.startswith("install")]:
    log = read(J("vm", d, "serial.log"))
    a = re.search(r"\[\s*([0-9.]+)\][^\n]*wolos-install: prepare", log)
    b = re.search(r"\[\s*([0-9.]+)\][^\n]*wolos-install: done", log)
    if a and b and float(b.group(1)) >= float(a.group(1)):
        install_s = round(float(b.group(1)) - float(a.group(1)), 1)
        evidence["install_s"] = {"from": f"vm/{d}/serial.log", "command": wolvm_cmd(d, journeys[d][0]),
                                 "what": "kernel timestamps of 'wolos-install: prepare' and 'wolos-install: done'"}
        break

# ---- pages: 'shot page-*' steps of journey install-gui ----
pages = None
if "install-gui" in journeys:
    head, steps = journeys["install-gui"]
    names = {s["step"].split()[1] for s in steps if s.get("ok") and re.match(r"shot page-\S+", s.get("step", ""))}
    if names:
        pages = len(names)
        evidence["pages"] = {"from": "vm/install-gui/journey.jsonl", "command": wolvm_cmd("install-gui", head),
                             "what": "distinct 'shot page-*' steps"}

# ---- idle CPU: idle.py rows of the compositor and the shell ----
idle = None
for f in sorted(glob.glob(J("vm", "*", "idle.txt"))):
    rows = [l.split() for l in read(f).splitlines()[1:]]
    vals = [float(r[1]) for r in rows if len(r) >= 5 and r[0] in ("Hyprland", "wol-shell")]
    if vals:
        idle = round(sum(vals), 2)
        evidence["idle_cpu_ms_s"] = {"from": rel(f), "command": "python3 design/tools/perf/idle.py 60 Hyprland wol-shell",
                                     "what": "sum of the cpu ms/s column, Hyprland + wol-shell"}
        break

# ---- shell map: 'wol-shell: first map <Duration> at <ms>' (WOL_SHELL_TIMING=1) ----
def ms(dur):
    m = re.fullmatch(r"([0-9.]+)(ns|µs|us|ms|s)", dur)
    if not m:
        return None
    return float(m.group(1)) * {"ns": 1e-6, "µs": 1e-3, "us": 1e-3, "ms": 1.0, "s": 1000.0}[m.group(2)]

shell_map = None
for f in sorted(glob.glob(J("vm", "*", "*logs.txt"))) + sorted(glob.glob(J("vm", "*", "serial.log"))):
    m = re.search(r"wol-shell: first map (\S+)", read(f))
    if m and ms(m.group(1)) is not None:
        shell_map = round(ms(m.group(1)), 1)
        evidence["shell_map_ms"] = {"from": rel(f), "command": "WOL_SHELL_TIMING=1 wol-shell (journal of the session)",
                                    "what": "wol-shell: first map"}
        break

# ---- the VMs the journeys ran in (wolvm's vm.json of each) ----
vms = []
for f in sorted(glob.glob(J("vm", "*", "vms", "*", "vm.json"))):
    try:
        c = json.loads(read(f))
    except ValueError:
        continue
    name = c.get("name") or os.path.basename(os.path.dirname(f))
    vms.append({
        "name": name,
        "journeys": [d for d in dirs if journeys[d][0].get("vm") == name],
        "smp": c.get("smp"), "mem": c.get("mem"), "disk": c.get("disk"), "gpu": c.get("gpu"),
        "res": c.get("res"), "tpm": bool(c.get("tpm")), "secureboot": bool(c.get("secureboot")),
        "kvm": bool(c.get("kvm")), "boot": c.get("boot"),
    })

doc = {
    "schema": 1,
    "run": {
        "id": os.path.basename(run),
        "date": date or "1970-01-01",
        "version": version,
        "iso": os.path.basename(isos[0]) if isos else None,
        "sha256": rec.get("sha256"),
        "kernel": kernel,
    },
    "boot_s": boot_s,
    "boot_systemd_s": boot_systemd_s,
    "install_s": install_s,
    "pages": pages,
    "idle_cpu_ms_s": idle,
    "shell_map_ms": shell_map,
    "iso_bytes": iso_bytes,
    "journeys": {"total": len(jl), "passed": sum(1 for j in jl if j["passed"]), "list": jl},
    "vms": vms,
    "evidence": evidence,
    "webapps": None,
}


# ---- validate against the schema (the subset it uses) ----
def check(v, s, path, errs):
    t = s.get("type")
    if t is not None:
        ts = t if isinstance(t, list) else [t]
        ok = any({"null": v is None, "boolean": isinstance(v, bool), "object": isinstance(v, dict),
                  "array": isinstance(v, list), "string": isinstance(v, str),
                  "integer": isinstance(v, int) and not isinstance(v, bool),
                  "number": isinstance(v, (int, float)) and not isinstance(v, bool)}[x] for x in ts)
        if not ok:
            errs.append(f"{path}: {v!r} is not {t}")
            return
    if "const" in s and v != s["const"]:
        errs.append(f"{path}: must be {s['const']!r}")
    if isinstance(v, (int, float)) and not isinstance(v, bool) and "minimum" in s and v < s["minimum"]:
        errs.append(f"{path}: below {s['minimum']}")
    if isinstance(v, str):
        if len(v) < s.get("minLength", 0):
            errs.append(f"{path}: too short")
        if "pattern" in s and not re.search(s["pattern"], v):
            errs.append(f"{path}: {v!r} does not match {s['pattern']}")
    if isinstance(v, dict):
        for k in s.get("required", []):
            if k not in v:
                errs.append(f"{path}: missing {k}")
        props = s.get("properties", {})
        extra = s.get("additionalProperties", True)
        for k, x in v.items():
            if k in props:
                check(x, props[k], f"{path}.{k}", errs)
            elif extra is False:
                errs.append(f"{path}: unknown key {k}")
            elif isinstance(extra, dict):
                check(x, extra, f"{path}.{k}", errs)
    if isinstance(v, list) and "items" in s:
        for i, x in enumerate(v):
            check(x, s["items"], f"{path}[{i}]", errs)


errs = []
check(doc, json.loads(read(schema_path)), "metrics", errs)
text = json.dumps(doc, indent=2, ensure_ascii=False) + "\n"
if out == "-":
    sys.stdout.write(text)
else:
    with open(out, "w", encoding="utf-8") as f:
        f.write(text)
for e in errs:
    print("metrics: " + e, file=sys.stderr)
sys.exit(1 if errs else 0)
PY
