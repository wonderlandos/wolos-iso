#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Summarise tests/e2e.sh output: OUT/<journey>/run<N>/result.json + journey.jsonl -> index.html,
summary.md, summary.json. Verdict per journey: PASS (every run clean), FAIL (every run failed), FLAKE
(some passed and some failed). Steps over 1 s are listed per journey; SLOW means over its own budget."""
import html, json, os, sys

out, runs = os.path.abspath(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 1
SLOW_S = 1.0


def load(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def steps(d):
    rows = []
    try:
        for line in open(os.path.join(d, "journey.jsonl"), encoding="utf-8"):
            r = json.loads(line)
            if "journey" in r:
                rows = []
            else:
                rows.append(r)
    except (OSError, ValueError):
        pass
    return rows


journeys = []
for name in sorted(x for x in os.listdir(out) if os.path.isdir(os.path.join(out, x))):
    rs = []
    for n in range(1, runs + 1):
        d = os.path.join(out, name, f"run{n}")
        if not os.path.isdir(d):
            continue
        res = load(os.path.join(d, "result.json"))
        rows = steps(d)
        ok = bool(res) and res["failed"] == 0
        rs.append({"run": n, "dir": d, "ok": ok, "no_result": res is None, "res": res, "steps": rows})
    if not rs:
        continue
    npass = sum(1 for r in rs if r["ok"])
    verdict = "PASS" if npass == len(rs) else ("FAIL" if npass == 0 else "FLAKE")
    over = {}
    for r in rs:
        for s in r["steps"]:
            if s["secs"] > SLOW_S and s["step"].split(" ")[0] not in ("start", "wait-agent", "wait-stable", "sleep", "type", "snapshot-restore", "stop", "wait-ui", "wait-cmd", "wait-screen", "wait-text"):
                over.setdefault(s["n"], {"step": s["step"], "secs": []})["secs"].append(s["secs"])
    journeys.append({"name": name, "verdict": verdict, "runs": [
        {"run": r["run"], "ok": r["ok"], "steps": (r["res"] or {}).get("steps"), "failed": (r["res"] or {}).get("failed"),
         "slow": (r["res"] or {}).get("slow"), "secs": (r["res"] or {}).get("secs"),
         "first_failure": (r["res"] or {}).get("first_failure"), "soft": (r["res"] or {}).get("soft") or [], "no_result": r["no_result"]} for r in rs],
        "over_1s": [{"n": k, **v} for k, v in sorted(over.items())]})
    journeys[-1]["_rs"] = rs

def contact_sheet(d, rows, cols=4, tw=320):
    """One PNG of every step's picture, numbered, so a run is reviewed in a glance (needs Pillow)."""
    try:
        from PIL import Image, ImageDraw
    except ImportError:
        return None
    shots = [(r["n"], r["ok"], os.path.join(d, r["shot"])) for r in rows if r.get("shot") and os.path.exists(os.path.join(d, r["shot"]))]
    if not shots:
        return None
    th = tw * 5 // 8
    n = (len(shots) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * tw, n * (th + 14)), (17, 17, 17))
    dr = ImageDraw.Draw(sheet)
    for i, (num, ok, p) in enumerate(shots):
        try:
            im = Image.open(p).convert("RGB").resize((tw - 4, th - 2))
        except OSError:
            continue
        x, y = (i % cols) * tw, (i // cols) * (th + 14)
        sheet.paste(im, (x + 2, y + 14))
        dr.text((x + 4, y + 1), f"{num:02d}" + ("" if ok else " FAIL"), fill=(255, 255, 255) if ok else (255, 90, 90))
    path = os.path.join(d, "contact.png")
    sheet.save(path)
    return path


for j in journeys:
    for r in j["_rs"]:
        contact_sheet(r["dir"], r["steps"])

counts = {v: sum(1 for j in journeys if j["verdict"] == v) for v in ("PASS", "FAIL", "FLAKE")}
with open(os.path.join(out, "summary.json"), "w") as f:
    json.dump({"out": out, "runs": runs, "counts": counts,
               "journeys": [{k: v for k, v in j.items() if k != "_rs"} for j in journeys]}, f, indent=1)

md = [f"# E2E summary ({runs} run(s) per journey)", "",
      f"PASS {counts['PASS']}, FAIL {counts['FAIL']}, FLAKE {counts['FLAKE']}", "",
      "| journey | verdict | runs | first failure |", "|---|---|---|---|"]
e = html.escape
h = ["<!doctype html><meta charset=utf-8><title>WolOS e2e</title><style>body{font:14px sans-serif;max-width:1300px;"
     "margin:2em auto;background:#111;color:#ddd}table{border-collapse:collapse;width:100%}td,th{border:1px solid #333;"
     "padding:.4em .6em;text-align:left;vertical-align:top}.PASS{color:#6c6}.FAIL{color:#f66;font-weight:bold}"
     ".FLAKE{color:#fc4;font-weight:bold}a{color:#8cf}small{color:#999}pre{white-space:pre-wrap;margin:0}</style>",
     f"<h1>WolOS end-to-end journeys</h1><p>{runs} run(s) per journey on copies of the kept v0.12 install. "
     f"<b class=PASS>PASS {counts['PASS']}</b> <b class=FAIL>FAIL {counts['FAIL']}</b> <b class=FLAKE>FLAKE {counts['FLAKE']}</b></p>",
     "<table><tr><th>journey</th><th>verdict</th><th>runs</th><th>steps over 1 s</th><th>first failure</th></tr>"]
for j in journeys:
    cells, ff = [], ""
    for r in j["runs"]:
        d = f"{j['name']}/run{r['run']}"
        lab = "ok" if r["ok"] else ("no result" if r["no_result"] else f"{r['failed']} failed")
        cells.append(f"<a href='{e(d)}/index.html'>run {r['run']}: {lab}</a> (<a href='{e(d)}/contact.png'>sheet</a>) <small>{r['steps'] or 0} steps, {r['secs'] or 0} s, {r['slow'] or 0} slow</small>")
        if r["first_failure"] and not ff:
            x = r["first_failure"]
            ff = f"step {x['n']} <code>{e(x['step'])}</code><pre>{e(x['detail'])}</pre>"
    softs = {}
    for r in j["runs"]:
        for x in r["soft"]:
            softs.setdefault(x["n"], x)
    if softs:
        ff += "".join(f"<p><b>defect (soft) step {x['n']}</b> <code>{e(x['step'][:60])}</code><pre>{e(x['detail'])}</pre></p>" for x in softs.values())
    over = "<br>".join(f"{o['n']:02d} {e(o['step'][:50])}: {', '.join(f'{v:.2f}' for v in o['secs'])} s" for o in j["over_1s"]) or "-"
    h.append(f"<tr><td>{e(j['name'])}</td><td class={j['verdict']}>{j['verdict']}</td><td>{'<br>'.join(cells)}</td><td>{over}</td><td>{ff}</td></tr>")
    first = next((r["first_failure"] for r in j["runs"] if r["first_failure"]), None)
    md.append(f"| {j['name']} | {j['verdict']} | " + ", ".join(f"run {r['run']} {'ok' if r['ok'] else 'FAIL'}" for r in j["runs"]) +
              f" | {('step %d %s' % (first['n'], first['step'])) if first else ''} |")
h.append("</table>")
with open(os.path.join(out, "index.html"), "w") as f:
    f.write("\n".join(h) + "\n")
with open(os.path.join(out, "summary.md"), "w") as f:
    f.write("\n".join(md) + "\n")
print(f"PASS {counts['PASS']} FAIL {counts['FAIL']} FLAKE {counts['FLAKE']}")
