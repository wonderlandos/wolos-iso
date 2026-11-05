#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Unit tests for wolvm: step parser, key/QMP builders, qemu command, PNG writer, report.
Run: python3 tests/wolvm/test_wolvm.py   (no QEMU needed; also runs on Windows)."""
import contextlib, io, json, os, shutil, struct, subprocess, sys, tempfile, unittest, unittest.mock, zlib
from importlib.machinery import SourceFileLoader
sys.dont_write_bytecode = True  # no __pycache__ next to the wolvm script
from importlib.util import module_from_spec, spec_from_loader

HERE = os.path.dirname(os.path.abspath(__file__))
_loader = SourceFileLoader("wolvm", os.path.join(HERE, "wolvm"))
_spec = spec_from_loader("wolvm", _loader)
W = module_from_spec(_spec)
_loader.exec_module(W)

CFG = {"name": "t", "iso": "/isos/wolos.iso", "mem": "6G", "smp": 4, "tpm": False, "secureboot": False,
       "gpu": "virtio", "res": "1280x800", "boot": "cdrom", "append": "", "creds": [], "ssh": False,
       "fw_code": "/fw/OVMF_CODE.4m.fd", "kvm": True}


def put(path, data):
    """Write data (str or bytes) to path and close it (no ResourceWarning)."""
    with open(path, "wb" if isinstance(data, bytes) else "w") as f:
        f.write(data)


def get(path):
    with open(path) as f:
        return f.read()


def opt_after(cmd, flag):
    return [cmd[i + 1] for i, a in enumerate(cmd) if a == flag]


class Steps(unittest.TestCase):
    def test_string_forms(self):
        self.assertEqual(W.parse_step("key ctrl-alt-t ret"), {"verb": "key", "args": ["ctrl-alt-t", "ret"], "opts": {}})
        self.assertEqual(W.parse_step("type hello world")["args"], ["hello world"])
        self.assertEqual(W.parse_step('type "a\\nb"')["args"], ["a\nb"])
        self.assertEqual(W.parse_step("click 10 20")["args"], ["10", "20"])
        self.assertEqual(W.parse_step("start")["args"], [])
        self.assertEqual(W.parse_step("shot")["args"], [])

    def test_dict_form(self):
        st = W.parse_step({"wait-text": "login:", "timeout": 90})
        self.assertEqual(st, {"verb": "wait-text", "args": ["login:"], "opts": {"timeout": 90}})
        self.assertEqual(W.parse_step({"click": [5, 6]})["args"], ["5", "6"])
        self.assertEqual(W.parse_step({"wait-text": "x", "shot": False})["opts"], {"shot": False})

    def test_errors(self):
        for bad in ("dance now", {"timeout": 3, "key": "a"}, {}, "type", 7):
            with self.assertRaises(ValueError, msg=repr(bad)):
                W.parse_step(bad)

    def test_yaml_subset(self):
        text = """# a journey
name: live boot
vm: wolos
steps:
- start
- wait-text: "login:"
  timeout: 300
  regex: false
- type: "root\\n"
- key: ctrl-alt-t
- ssh: 'systemctl is-system-running'
  expect: running
  retry: 2
- shot desktop
"""
        meta, items = W.parse_yaml_steps(text)
        self.assertEqual(meta, {"name": "live boot", "vm": "wolos"})
        steps = [W.parse_step(i) for i in items]
        self.assertEqual([s["verb"] for s in steps], ["start", "wait-text", "type", "key", "ssh", "shot"])
        self.assertEqual(steps[1]["opts"], {"timeout": 300, "regex": False})
        self.assertEqual(steps[2]["args"], ["root\n"])
        self.assertEqual(steps[4]["opts"], {"expect": "running", "retry": 2})
        self.assertEqual(steps[5]["args"], ["desktop"])

    def test_load_json_and_examples(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "j.json")
            put(p, json.dumps({"name": "x", "steps": ["start", {"sleep": 1}]}))
            meta, steps = W.load_journey(p)
            self.assertEqual(meta, {"name": "x"})
            self.assertEqual(steps[1], {"verb": "sleep", "args": ["1"], "opts": {}})
        for f in os.listdir(os.path.join(HERE, "journeys")):
            meta, steps = W.load_journey(os.path.join(HERE, "journeys", f))
            self.assertTrue(steps, f)


class Keys(unittest.TestCase):
    def test_combo(self):
        self.assertEqual(W.combo_keys("ctrl-alt-t"), ["ctrl", "alt", "t"])
        self.assertEqual(W.combo_keys("Super-Enter"), ["meta_l", "ret"])
        self.assertEqual(W.combo_keys("-"), ["minus"])
        with self.assertRaises(ValueError):
            W.combo_keys("ctrl--")

    def test_text(self):
        self.assertEqual(W.text_keys("aB:\n"), [["a"], ["shift", "b"], ["shift", "semicolon"], ["ret"]])
        self.assertEqual(W.text_keys('"x 1'), [["shift", "apostrophe"], ["x"], ["spc"], ["1"]])
        with self.assertRaises(ValueError):
            W.text_keys("é")

    def test_qmp_builders(self):
        self.assertEqual(W.qmp_send_key(["ctrl", "c"]), {"execute": "send-key", "arguments": {
            "keys": [{"type": "qcode", "data": "ctrl"}, {"type": "qcode", "data": "c"}], "hold-time": 40}})
        m = W.qmp_abs_move(1279, 0, 1280, 800)["arguments"]["events"]
        self.assertEqual((m[0]["data"]["value"], m[1]["data"]["value"]), (32767, 0))
        self.assertEqual(W.qmp_abs_move(5000, -3, 1280, 800)["arguments"]["events"][0]["data"]["value"], 32767)
        self.assertEqual(W.qmp_button("left", True)["arguments"]["events"][0]["data"], {"button": "left", "down": True})
        self.assertEqual(W.qmp_screendump("/a.png"), {"execute": "screendump", "arguments": {"filename": "/a.png", "format": "png"}})
        self.assertNotIn("format", W.qmp_screendump("/a.ppm", "ppm")["arguments"])
        self.assertEqual(W.qmp_hmp("savevm x")["arguments"], {"command-line": "savevm x"})


class QemuCmd(unittest.TestCase):
    def test_cdrom_first_with_console(self):
        c = W.qemu_cmd(CFG, "/vm")
        self.assertIn("virtio-blk-pci,drive=disk0,bootindex=1", c)
        self.assertIn("ide-cd,id=cd0,drive=cdrom0,bootindex=0", c)
        smb = opt_after(c, "-smbios")
        self.assertIn("type=11,value=io.systemd.boot.kernel-cmdline-extra=console=tty0 console=ttyS0,,115200", smb)
        self.assertIn("-daemonize", c)
        self.assertEqual(opt_after(c, "-accel"), ["kvm"])
        self.assertIn("virtio-vga,xres=1280,yres=800", c)
        self.assertIn("virtserialport,chardev=qga0,name=org.qemu.guest_agent.0", c)
        self.assertNotIn("-tpmdev", c)

    def test_disk_boot_tpm_sb_ssh(self):
        cfg = dict(CFG, boot="disk", tpm=True, secureboot=True, gpu="vmsvga", kvm=False,
                   creds=["ssh.authorized_keys.root=ssh-ed25519 AAA x,y"])
        c = W.qemu_cmd(cfg, "/vm", {"ssh_port": 2233})
        self.assertIn("virtio-blk-pci,drive=disk0,bootindex=0", c)
        self.assertIn("q35,smm=on", c)
        self.assertIn("driver=cfi.pflash01,property=secure,value=on", c)
        self.assertIn("vmware-svga", c)
        self.assertIn("emulator,id=tpm0,chardev=tpmc", c)
        self.assertEqual(opt_after(c, "-accel"), ["tcg,thread=multi"])
        smb = opt_after(c, "-smbios")  # the cmdline extra stays (ignored while enforced), then the credential
        self.assertEqual(smb[-1], "type=11,value=io.systemd.credential:ssh.authorized_keys.root=ssh-ed25519 AAA x,,y")
        self.assertIn("user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:2233-:22", c)

    def test_no_iso_and_direct(self):
        c = W.qemu_cmd(dict(CFG, iso=None), "/vm")
        self.assertNotIn("-cdrom", c)
        self.assertFalse(any("cdrom0" in a for a in c))
        d = dict(CFG, append="wolos.try", direct={"kernel": "/vm/k", "initrd": "/vm/i", "basedir": "wolos", "label": "WOLOS_202611"})
        c = W.qemu_cmd(d, "/vm")
        self.assertEqual(opt_after(c, "-append"),
                         ["archisobasedir=wolos archisolabel=WOLOS_202611 console=tty0 console=ttyS0,115200 wolos.try"])
        self.assertEqual(opt_after(c, "-kernel"), ["/vm/k"])
        self.assertFalse(any("bootindex" in a for a in c))  # -kernel owns bootindex 0
        self.assertFalse(any("wolos.try" in a for a in opt_after(c, "-smbios")))
        c = W.qemu_cmd(dict(d, boot="disk"), "/vm")  # after the install: the disk, no -kernel
        self.assertNotIn("-kernel", c)
        self.assertIn("virtio-blk-pci,drive=disk0,bootindex=0", c)

    def test_append_only_reaches_the_live_boot(self):
        live = dict(CFG, append="wolos.autoinstall=/dev/vda")
        self.assertTrue(any("wolos.autoinstall" in a for a in opt_after(W.qemu_cmd(live, "/vm"), "-smbios")))
        smb = opt_after(W.qemu_cmd(dict(live, boot="disk"), "/vm"), "-smbios")
        self.assertTrue(smb and not any("wolos.autoinstall" in a for a in smb))  # console only

    def test_extra_drives_raw_in_order(self):
        c = W.qemu_cmd(dict(CFG, drives=["/t/other.raw", "/t/keys.raw,ro", "/t/x.raw,snapshot"]), "/vm")
        drv = [a for a in opt_after(c, "-drive") if "id=extra" in a]
        self.assertEqual(drv, ["file=/t/other.raw,if=none,id=extra0,format=raw",
                               "file=/t/keys.raw,if=none,id=extra1,format=raw,readonly=on",
                               "file=/t/x.raw,if=none,id=extra2,format=raw,snapshot=on"])
        dev = opt_after(c, "-device")
        self.assertLess(dev.index("virtio-blk-pci,drive=disk0,bootindex=1"), dev.index("virtio-blk-pci,drive=extra0"))
        self.assertEqual(W.split_drive("/a,b.raw"), ("/a,b.raw", ""))

    def test_no_net(self):
        self.assertEqual(opt_after(W.qemu_cmd(dict(CFG, net=False), "/vm", {"ssh_port": 5}), "-nic"), ["none"])
        self.assertEqual(opt_after(W.qemu_cmd(CFG, "/vm"), "-nic"), ["user,model=virtio-net-pci"])

    def test_iso_bus_virtio(self):
        d = dict(CFG, iso_bus="virtio", direct={"kernel": "/vm/k", "initrd": "/vm/i", "basedir": "wolos", "label": "L"})
        c = W.qemu_cmd(d, "/vm")
        self.assertIn("file=/isos/wolos.iso,if=none,id=cdrom0,format=raw,snapshot=on", c)
        self.assertIn("virtio-blk-pci,id=cd0,drive=cdrom0", c)
        self.assertFalse(any(a.startswith("ide-cd") for a in c))
        self.assertEqual(opt_after(c, "-kernel"), ["/vm/k"])

    def test_vars_secureboot_cmds(self):
        on = W.vars_secureboot_cmds("/vm/vars.qcow2", True, "/vm/e")
        self.assertEqual(on[0], ["qemu-img", "convert", "-f", "qcow2", "-O", "raw", "/vm/vars.qcow2", "/vm/e.raw"])
        self.assertEqual(on[1], ["virt-fw-vars", "--input", "/vm/e.raw", "--output", "/vm/e.sb.raw", "--set-true", "SecureBootEnable"])
        self.assertEqual(on[2][-2:], ["/vm/e.sb.raw", "/vm/vars.qcow2"])
        self.assertIn("--set-false", W.vars_secureboot_cmds("/v", False, "/e")[1])

    def test_swtpm(self):
        c = W.swtpm_cmd("/vm")
        self.assertEqual(c[:3], ["swtpm", "socket", "--tpm2"])
        self.assertIn("type=unixio,path=" + os.path.join("/vm", "tpm.sock"), c)


class Images(unittest.TestCase):
    def test_ppm_to_png(self):
        ppm = b"P6\n2 1\n255\n" + bytes([255, 0, 0, 0, 255, 0])
        png, size = W.ppm_to_png(ppm)
        self.assertEqual(size, (2, 1))
        self.assertEqual(png[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(struct.unpack(">II", png[16:24]), (2, 1))
        idat = png.index(b"IDAT")
        n = struct.unpack(">I", png[idat - 4:idat])[0]
        self.assertEqual(zlib.decompress(png[idat + 4:idat + 4 + n]), b"\x00\xff\x00\x00\x00\xff\x00")
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.png")
            put(p, png)
            self.assertEqual(W.png_size(p), (2, 1))

    def test_iso_label(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.iso")
            pvd = bytearray(2048)
            pvd[0:6] = b"\x01CD001"
            pvd[40:72] = b"WOLOS_202611".ljust(32)
            put(p, bytes(32768) + bytes(pvd))
            self.assertEqual(W.iso_label(p), "WOLOS_202611")


class Report(unittest.TestCase):
    def test_report_from_journey(self):
        with tempfile.TemporaryDirectory() as d:
            with open(os.path.join(d, "journey.jsonl"), "w") as f:
                f.write(json.dumps({"journey": "old", "vm": "v", "time": "t"}) + "\n")
                f.write(json.dumps({"n": 1, "step": "stale", "ok": False, "shot": None, "detail": ""}) + "\n")
                f.write(json.dumps({"journey": "boot <live>", "vm": "v", "time": "t"}) + "\n")
                f.write(json.dumps({"n": 1, "step": "start", "ok": True, "secs": 1, "shot": "01-start.png", "detail": ""}) + "\n")
                f.write(json.dumps({"n": 2, "step": "wait-text x", "ok": False, "secs": 9, "shot": None, "detail": "timeout"}) + "\n")
            put(os.path.join(d, "serial.log"), "\x1b[0mhello serial\n")
            W.write_report(d)
            md = get(os.path.join(d, "report.md"))
            self.assertIn("1/2 steps ok", md)
            self.assertNotIn("stale", md)
            self.assertIn("![start](01-start.png)", md)
            self.assertIn("hello serial", md)
            h = get(os.path.join(d, "index.html"))
            self.assertIn("boot &lt;live&gt;", h)
            self.assertIn("<img src='01-start.png'", h)

    def test_report_closes_every_file(self):
        """write_report under python3 -X dev: no file left for the collector to close."""
        import gc, warnings
        with tempfile.TemporaryDirectory() as d:
            put(os.path.join(d, "journey.jsonl"), json.dumps({"journey": "j", "vm": "v", "time": "t"}) + "\n"
                + json.dumps({"n": 1, "step": "start", "ok": True, "secs": 1, "shot": None, "detail": ""}) + "\n")
            put(os.path.join(d, "serial.log"), "serial\n")
            with warnings.catch_warnings(record=True) as seen:
                warnings.simplefilter("always", ResourceWarning)
                W.write_report(d)
                gc.collect()
            self.assertEqual([str(w.message) for w in seen if w.category is ResourceWarning], [])


class DryRun(unittest.TestCase):
    def run_cli(self, *argv):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = W.main(list(argv))
        return rc, buf.getvalue()

    def test_new_prints_qemu(self):
        with tempfile.TemporaryDirectory() as d:
            fw = os.path.join(d, "fw")
            os.makedirs(fw)
            for n in ("OVMF_CODE.4m.fd", "OVMF_VARS.4m.fd"):
                put(os.path.join(fw, n), b"\0")
            iso = os.path.join(d, "x.iso")
            put(iso, b"\0")
            os.environ["WOLVM_OVMF"] = fw
            try:
                rc, out = self.run_cli("--home", d, "--dry-run", "new", "demo", "--iso", iso, "--tpm", "--vmsvga")
            finally:
                del os.environ["WOLVM_OVMF"]
            self.assertEqual(rc, 0)
            self.assertIn("qemu-system-x86_64 -name wolvm-demo", out)
            self.assertIn("vmware-svga", out)
            self.assertFalse(os.path.exists(os.path.join(d, "vms")))  # nothing created

    def make_fw(self, d):
        fw = os.path.join(d, "fw")
        os.makedirs(fw)
        for n in ("OVMF_CODE.4m.fd", "OVMF_CODE.secboot.4m.fd", "OVMF_VARS.4m.fd"):
            put(os.path.join(fw, n), b"\0")
        iso = os.path.join(d, "x.iso")
        put(iso, b"\0")
        return fw, iso

    def test_new_drive_nonet_isobus_dry(self):
        with tempfile.TemporaryDirectory() as d:
            fw, iso = self.make_fw(d)
            other = os.path.join(d, "other.raw")
            put(other, b"\0")
            os.environ["WOLVM_OVMF"] = fw
            try:
                rc, out = self.run_cli("--home", d, "--dry-run", "new", "t", "--iso", iso, "--drive", other,
                                       "--drive", other + ",ro", "--no-net", "--iso-bus", "virtio")
            finally:
                del os.environ["WOLVM_OVMF"]
            self.assertEqual(rc, 0)
            self.assertIn("other.raw,if=none,id=extra0,format=raw", out)
            self.assertNotIn("id=extra0,format=raw,", out)  # as it is: no ro, no snapshot
            self.assertIn("other.raw,if=none,id=extra1,format=raw,readonly=on", out)
            self.assertIn("-nic none", out)
            self.assertIn("x.iso,if=none,id=cdrom0,format=raw,snapshot=on", out)
            self.assertIn("virtio-blk-pci,id=cd0,drive=cdrom0", out)

    def test_set_secureboot_dry(self):
        with tempfile.TemporaryDirectory() as d:
            vm = os.path.join(d, "vms", "t")
            os.makedirs(vm)
            put(os.path.join(vm, "vm.json"), json.dumps(dict(CFG, secureboot=True)))
            rc, out = self.run_cli("--home", d, "--dry-run", "set", "t", "--secureboot", "off", "--net", "off")
            self.assertEqual(rc, 0)
            self.assertIn("virt-fw-vars --input", out)
            self.assertIn("--set-false SecureBootEnable", out)
            self.assertIn('"net": false', out)
            put(os.path.join(vm, "vm.json"), json.dumps(CFG))  # made without --secureboot: refused
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                self.run_cli("--home", d, "--dry-run", "set", "t", "--secureboot", "on")

    def test_ssh_open_dry(self):
        rc, out = self.run_cli("--dry-run", "ssh-open", "x")
        self.assertIn("nft insert rule inet wolos input tcp dport 22 accept", out)

    def test_journey_dry(self):
        rc, out = self.run_cli("--dry-run", "journey", "x", os.path.join(HERE, "journeys", "live-boot.yaml"))
        self.assertEqual(rc, 0)
        self.assertTrue(out.startswith("01 start"))


class Expect(unittest.TestCase):
    """I6-4: an ssh step's expect is an assertion: a mismatch fails the step."""
    def step(self, answer, opts):
        old = W.guest
        W.guest = lambda vm, cmd, t=300, user="root", via="auto": answer
        try:
            with tempfile.TemporaryDirectory() as d:
                return W.run_step(None, {"verb": "ssh", "args": ["x"], "opts": opts}, d, 1)[0]
        finally:
            W.guest = old

    def test_expect(self):
        self.assertTrue(self.step((0, "running", ""), {"expect": "running"}))
        self.assertFalse(self.step((0, "degraded", ""), {"expect": "running"}))
        self.assertFalse(self.step((1, "running", ""), {"expect": "running"}))
        self.assertTrue(self.step((1, "x", ""), {"rc": "any"}))

    def test_boot_journeys_assert(self):
        for f, want in (("live-boot.yaml", ["running", "no-failed-units", "services-up"]),
                        ("install-auto.yaml", ["rootflags=subvol=@wolos-a", "Current Entry: wolos-a",
                                               "running", "no-failed-units", "wolos-a.conf",
                                               "bind-ok", "no-failed-units-again",
                                               "second-start-unlocked", "success"])):
            _, steps = W.load_journey(os.path.join(HERE, "journeys", f))
            got = [st["opts"]["expect"] for st in steps if "expect" in st["opts"]]
            for w in want:
                self.assertIn(w, got, f)

    def test_install_auto_starts_the_installed_disk_twice(self):
        """I6-4: the second start (the re-bound policy) is walked, and nobody types in it."""
        _, steps = W.load_journey(os.path.join(HERE, "journeys", "install-auto.yaml"))
        verbs = [st["verb"] for st in steps]
        starts = [i for i, v in enumerate(verbs) if v == "start"]
        self.assertEqual(len(starts), 3, verbs)
        after = verbs[starts[-1]:]
        self.assertEqual(after[0], "start")
        self.assertIn("wait-agent", after)
        self.assertNotIn("type", after)
        self.assertNotIn("key", after)
        # the stop before the second start, with the checkpoint taken first
        self.assertEqual(verbs[starts[-1] - 1], "stop")
        self.assertLess(verbs.index("checkpoint"), starts[-1])
        # the reseal unit is asked on the second start
        seen = [st["args"][0] for st in steps[starts[-1]:] if st["verb"] == "ssh"]
        self.assertTrue(any("wolos-tpm-reseal" in a for a in seen), seen)


class StopAfterPoweroff(unittest.TestCase):
    """v0.12: the guest powered itself off ("reboot: Power down"), so the monitor of the dying QEMU
    resets; `stop` must treat that as already going down and still end in a clean "stopped"."""

    class ResetOnConnect:
        def __init__(self):
            raise ConnectionResetError(104, "Connection reset by peer")

    class ResetOnFirstCall:
        calls = []

        def call(self, msg):
            self.calls.append(msg["execute"])
            raise ConnectionResetError(104, "Connection reset by peer")

    def stop(self, qmp_factory, via_step=False):
        pids = iter([4242, 4242])  # running at the first look and while waiting, then gone
        killed = []
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W.VM, "pid", lambda self: next(pids, None)), \
                unittest.mock.patch.object(W.VM, "qmp", lambda self: qmp_factory()), \
                unittest.mock.patch.object(W.time, "sleep", lambda s: None), \
                unittest.mock.patch.object(W.os, "kill", lambda *a: killed.append(a)):
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                if via_step:
                    vm = W.VM(d, "t")
                    ok, _ = W.run_step(vm, W.parse_step({"stop": "", "timeout": 5}), d, 0)
                    rc = 0 if ok else 1
                else:
                    rc = W.cmd_stop(W.argparse.Namespace(home=d, name="t", force=False, timeout=5))
        self.assertEqual(killed, [])
        return rc, buf.getvalue()

    def test_reset_on_connect(self):
        rc, out = self.stop(self.ResetOnConnect)
        self.assertEqual(rc, 0)
        self.assertTrue(out.rstrip().endswith("t: stopped"), out)

    def test_reset_on_first_call(self):
        self.ResetOnFirstCall.calls = []
        rc, out = self.stop(self.ResetOnFirstCall)
        self.assertEqual(rc, 0)
        self.assertEqual(self.ResetOnFirstCall.calls, ["system_powerdown"])
        self.assertTrue(out.rstrip().endswith("t: stopped"), out)

    def test_journey_stop_step_takes_the_same_path(self):
        rc, out = self.stop(self.ResetOnFirstCall, via_step=True)
        self.assertEqual(rc, 0)
        self.assertTrue(out.rstrip().endswith("t: stopped"), out)


class BlockScalars(unittest.TestCase):
    def test_item_and_option_blocks(self):
        text = """steps:
- ssh: |
    a=1
      indented
    echo "$a"

  expect: "1"
  save: |
    out.txt
- shot done
"""
        meta, items = W.parse_yaml_steps(text)
        st = W.parse_step(items[0])
        self.assertEqual(st["args"], ['a=1\n  indented\necho "$a"'])
        self.assertEqual(st["opts"], {"expect": "1", "save": "out.txt"})
        self.assertEqual(W.parse_step(items[1])["args"], ["done"])

    def test_crash_hotplug_journey_parses(self):
        _, steps = W.load_journey(os.path.join(HERE, "journeys", "qa100-crash-hotplug-panels.yaml"))
        matrix = [s for s in steps if s["opts"].get("save") == "hotplug-matrix.txt"][0]
        self.assertIn("for p in control-center", matrix["args"][0])
        self.assertTrue(matrix["args"][0].endswith('echo "deaths=$dead"'))
        self.assertEqual(matrix["opts"]["expect"], "deaths=0")


class NewVerbs(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(W.parse_step("plug usb-storage /i/stick.img")["args"], ["usb-storage", "/i/stick.img"])
        self.assertEqual(W.parse_step("display 2 on")["args"], ["2", "on"])
        self.assertEqual(W.parse_step({"frames": None, "every": 100, "for": 3})["opts"], {"every": 100, "for": 3})
        self.assertEqual(W.parse_step("suspend")["args"], [])
        self.assertEqual(W.parse_step({"power-cut": "files 15", "delay": 20})["args"], ["files 15"])
        self.assertEqual(W.parse_step({"qmp": '{"execute": "query-status"}'})["args"], ['{"execute": "query-status"}'])
        for bad in ("plug", "unplug", "power-cut", "disk-edit", "qmp"):
            with self.assertRaises(ValueError, msg=bad):
                W.parse_step(bad)

    def test_qmp_parse(self):
        self.assertEqual(W.qmp_parse("system_wakeup"), {"execute": "system_wakeup"})
        self.assertEqual(W.qmp_parse('device_del {"id": "x"}'), {"execute": "device_del", "arguments": {"id": "x"}})
        self.assertEqual(W.qmp_parse("device_del", '{"id": "y"}')["arguments"], {"id": "y"})
        self.assertEqual(W.qmp_parse('{"execute": "quit"}'), {"execute": "quit"})
        for bad in ('{"x": 1}', "Bad Cmd", "rm;-rf"):
            with self.assertRaises(ValueError, msg=bad):
                W.qmp_parse(bad)

    def test_plug_msgs(self):
        m = W.plug_msgs("usb-storage", "stick0", "/i/s.img", ro=True)
        self.assertEqual(m[0]["execute"], "blockdev-add")
        self.assertEqual(m[0]["arguments"]["node-name"], "stick0-blk")
        self.assertEqual(m[0]["arguments"]["file"], {"driver": "file", "filename": "/i/s.img", "read-only": True})
        self.assertEqual(m[1]["arguments"], {"driver": "usb-storage", "id": "stick0", "bus": "xhci.0",
                                             "drive": "stick0-blk", "removable": True})
        a = W.plug_msgs("usb-audio", "a0")
        self.assertEqual(a, [{"execute": "device_add", "arguments": {"driver": "usb-audio", "id": "a0",
                                                                     "bus": "xhci.0", "audiodev": "snd0"}}])
        with self.assertRaises(ValueError):
            W.plug_msgs("usb-storage", "s")  # no image
        with self.assertRaises(ValueError):
            W.plug_msgs("pci-bomb", "s")
        with self.assertRaises(ValueError):
            W.plug_msgs("usb-audio", "bad id")
        self.assertEqual([x["execute"] for x in W.unplug_msgs("stick0", "usb-storage")], ["device_del", "blockdev-del"])
        self.assertEqual(W.unplug_msgs("a0", "usb-audio"), [{"execute": "device_del", "arguments": {"id": "a0"}}])

    def test_display_cmd_and_stage(self):
        c = W.display_cmd(2, True)
        self.assertIn("Virtual-2/force", c)
        self.assertIn("echo on >", c)
        self.assertIn("udevadm trigger --action=change --subsystem-match=drm", c)
        self.assertIn("echo off >", W.display_cmd(3, False))
        with self.assertRaises(ValueError):
            W.display_cmd(0, True)
        log = "x\nwolos-install: disk 5\nnoise\nwolos-install: files 15 Copying WolOS\n"
        self.assertEqual(W.last_install_stage(log), "wolos-install: files 15 Copying WolOS")
        self.assertEqual(W.last_install_stage("nothing"), "")

    def test_heads(self):
        two = dict(CFG, displays=2)
        self.assertEqual(W.head_target(CFG, None), (None, None))
        self.assertEqual(W.head_target(CFG, 0), (None, None))
        self.assertEqual(W.head_target(two, 1), ("vga0", 1))
        with self.assertRaises(ValueError):
            W.head_target(two, 2)
        self.assertEqual(W.shot_heads(two, "all"), [0, 1])
        self.assertEqual(W.shot_heads(CFG, "all"), [None])
        self.assertEqual(W.shot_heads(two, None), [None])
        self.assertEqual(W.shot_heads(two, "0, 1"), [0, 1])
        self.assertEqual(W.qmp_screendump("/a.png", "png", "vga0", 1)["arguments"],
                         {"filename": "/a.png", "format": "png", "device": "vga0", "head": 1})


class HardwareCmd(unittest.TestCase):
    def test_defaults_unchanged_but_hotplug_ready(self):
        c = W.qemu_cmd(CFG, "/vm")
        self.assertIn("qemu-xhci,id=xhci", c)
        self.assertEqual(opt_after(c, "-audiodev"), ["none,id=snd0"])
        self.assertFalse(any("hda" in a for a in c))  # no sound card by default
        self.assertNotIn("ICH9-LPC.disable_s3=0", c)
        self.assertFalse(any("max_outputs" in a for a in c))

    def test_audio(self):
        dev = opt_after(W.qemu_cmd(dict(CFG, audio=2), "/vm"), "-device")
        self.assertIn("intel-hda,id=hda0", dev)
        self.assertIn("hda-duplex,bus=hda0.0,audiodev=snd0", dev)
        self.assertIn("ich9-intel-hda,id=hda1", dev)
        self.assertIn("hda-output,bus=hda1.0,audiodev=snd0", dev)
        self.assertFalse(any("hda2" in a for a in dev))

    def test_displays(self):
        c = W.qemu_cmd(dict(CFG, displays=3), "/vm")
        self.assertIn("virtio-vga,id=vga0,max_outputs=3,xres=1280,yres=800", c)
        smb = opt_after(c, "-smbios")[0]
        self.assertIn("video=Virtual-2:1280x800e video=Virtual-3:1280x800e", smb)
        self.assertNotIn("Virtual-1", smb)
        gl = W.qemu_cmd(dict(CFG, displays=2, gpu="virtio-gl", boot="disk"), "/vm")
        self.assertIn("virtio-vga-gl,id=vga0,max_outputs=2,xres=1280,yres=800", gl)
        self.assertIn("video=Virtual-2:1280x800e", opt_after(gl, "-smbios")[0])  # the installed boot too
        with self.assertRaises(ValueError):
            W.qemu_cmd(dict(CFG, displays=2, gpu="vmsvga"), "/vm")

    def test_s3_and_extra_args(self):
        c = W.qemu_cmd(dict(CFG, s3=True, qemu_args=["-device usb-kbd", "-global foo.bar=1"]), "/vm")
        self.assertEqual(c[c.index("ICH9-LPC.disable_s3=0") - 1], "-global")
        i = c.index("-daemonize")
        self.assertEqual(c[i - 4:i], ["-device", "usb-kbd", "-global", "foo.bar=1"])

    def test_disk_buses(self):
        c = W.qemu_cmd(dict(CFG, disk_bus="nvme", disk_lbs=4096, disk_pbs=4096), "/vm")
        self.assertIn("nvme,drive=disk0,serial=WOLNVME0,logical_block_size=4096,physical_block_size=4096,bootindex=1", c)
        self.assertNotIn("ich9-ahci,id=ahci0", c)
        c = W.qemu_cmd(dict(CFG, disk_bus="sata", boot="disk"), "/vm")
        self.assertIn("ich9-ahci,id=ahci0", c)
        self.assertIn("ide-hd,drive=disk0,bus=ahci0.0,bootindex=0", c)
        c = W.qemu_cmd(dict(CFG, extra_bus="sata", extra_pbs=4096, drives=["/t/w.raw"]), "/vm")
        dev = opt_after(c, "-device")
        self.assertLess(dev.index("ich9-ahci,id=ahci0"), dev.index("ide-hd,drive=extra0,bus=ahci0.1,physical_block_size=4096"))
        self.assertIn("virtio-blk-pci,drive=disk0,bootindex=1", dev)
        self.assertNotIn("ich9-ahci,id=ahci0", W.qemu_cmd(dict(CFG, extra_bus="sata"), "/vm"))  # no extra disk
        with self.assertRaises(ValueError):
            W.blk_device("scsi", "d", 0)

    def test_disk_edit_plan(self):
        att, det = W.disk_edit_plan("/vm/disk.qcow2", "/dev/nbd1", "/vm/mnt", "wolvm-t")
        self.assertEqual(att[0], ["qemu-nbd", "--connect", "/dev/nbd1", "--format", "qcow2", "/vm/disk.qcow2"])
        self.assertIn(["mount", "/dev/nbd1p1", "/vm/mnt/esp"], att)
        self.assertIn(["mount", "-o", "subvol=@var", "/dev/nbd1p2", "/vm/mnt/var"], att)
        self.assertFalse(any(c[0] == "cryptsetup" for c in att + det))
        self.assertEqual(det[0], ["sync"])
        self.assertEqual(det[-1], ["qemu-nbd", "--disconnect", "/dev/nbd1"])
        att, det = W.disk_edit_plan("/d", "/dev/nbd0", "/m", "wolvm-t", "/k", read_only=True, luks=True)
        self.assertIn("--read-only", att[0])
        self.assertIn(["cryptsetup", "open", "--readonly", "--key-file", "/k", "/dev/nbd0p2", "wolvm-t"], att)
        self.assertIn(["mount", "-o", "ro,subvol=@wolos-a", "/dev/mapper/wolvm-t", "/m/slot"], att)
        self.assertIn(["cryptsetup", "close", "wolvm-t"], det)
        self.assertLess(det.index(["umount", "/m/esp"]), det.index(["cryptsetup", "close", "wolvm-t"]))

    def test_disk_edit_refuses_a_running_vm(self):
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W.VM, "pid", lambda self: 4242):
            with self.assertRaises(RuntimeError):
                W.disk_edit(W.VM(d, "t"), "true")

    def test_disk_edit_always_detaches(self):
        """The command fails, the mounts and the nbd still go (no device left behind)."""
        ran = []

        def fake_sh(cmd, **kw):
            ran.append(cmd)
            rc = 7 if "sh" in cmd and "-c" in cmd else 0
            out = "btrfs" if "blkid" in cmd else ""
            return subprocess.CompletedProcess(cmd, rc, out, "")
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W.VM, "pid", lambda self: None), \
                unittest.mock.patch.object(W, "sh", fake_sh), \
                unittest.mock.patch.object(W.shutil, "which", lambda t: "/usr/bin/" + t), \
                unittest.mock.patch.object(W.os.path, "exists", lambda p: p == "/dev/nbd0"), \
                unittest.mock.patch.object(W.os, "geteuid", lambda: 0), \
                unittest.mock.patch.object(W, "snapshot", lambda vm, act, tag=None: f"{act} {tag}"):
            rc, text = W.disk_edit(W.VM(d, "t"), "exit 7")
        self.assertEqual(rc, 7)
        self.assertIn("undo: wolvm snapshot t restore pre-edit-", text)
        flat = [" ".join(c) for c in ran]
        self.assertTrue(flat[-1].startswith("qemu-nbd --disconnect /dev/nbd0"), flat[-3:])
        self.assertTrue(any(f.startswith("umount ") and f.endswith("/esp") for f in flat))
        self.assertFalse(any("cryptsetup" in f for f in flat))
        env = [c for c in ran if c[0] == "env"][0]
        self.assertTrue(any(a.startswith("VAR=") and a.endswith(os.path.join("mnt", "var")) for a in env))


class FakeQmp:
    def __init__(self, replies=None):
        self.sent, self.replies = [], replies or {}

    def call(self, msg):
        self.sent.append(msg)
        r = self.replies.get(msg["execute"], {})
        return r.pop(0) if isinstance(r, list) else r


class StepsRun(unittest.TestCase):
    def vm(self, d, cfg=None, pid=4242):
        vm = W.VM(d, "t")
        os.makedirs(vm.dir, exist_ok=True)
        put(vm.cfg_path, json.dumps(cfg or dict(CFG, s3=True, displays=2)))
        return vm

    def test_qmp_step(self):
        q = FakeQmp({"query-status": {"status": "running"}})
        with tempfile.TemporaryDirectory() as d, unittest.mock.patch.object(W.VM, "qmp", lambda self: q):
            vm = self.vm(d)
            ok, detail = W.run_step(vm, W.parse_step({"qmp": "query-status", "expect": "running"}), d, 1)
            self.assertTrue(ok, detail)
            ok, _ = W.run_step(vm, W.parse_step({"qmp": "query-status", "expect": "paused"}), d, 2)
            self.assertFalse(ok)

    def test_plug_unplug_storage(self):
        q = FakeQmp({"qom-list": [[{"name": "stick0"}], [{"name": "usb-tablet"}]]})
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W.VM, "qmp", lambda self: q), \
                unittest.mock.patch.object(W.time, "sleep", lambda s: None):
            vm = self.vm(d)
            img = os.path.join(d, "s.img")
            put(img, b"\0" * 512)
            ok, detail = W.run_step(vm, W.parse_step({"plug": f"usb-storage {img}", "id": "stick0"}), d, 1)
            self.assertTrue(ok, detail)
            self.assertEqual(vm.run["plugged"], {"stick0": "usb-storage"})
            ok, detail = W.run_step(vm, W.parse_step("unplug stick0"), d, 2)
            self.assertTrue(ok, detail)
            self.assertEqual([m["execute"] for m in q.sent],
                             ["blockdev-add", "device_add", "device_del", "qom-list", "qom-list", "blockdev-del"])
            self.assertEqual(vm.run["plugged"], {})

    def test_suspend_wake(self):
        q = FakeQmp({"query-status": [{"status": "running"}, {"status": "suspended"}, {"status": "running"}]})

        class Ag:
            calls = []

            def call(self, cmd, **a):
                self.calls.append((cmd, a))
                return {"pid": 1}
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W.VM, "qmp", lambda self: q), \
                unittest.mock.patch.object(W.VM, "pid", lambda self: 4242), \
                unittest.mock.patch.object(W.VM, "agent", lambda self, timeout=10: Ag()), \
                unittest.mock.patch.object(W.time, "sleep", lambda s: None):
            vm = self.vm(d)
            ok, detail = W.run_step(vm, W.parse_step("suspend"), d, 1)
            self.assertTrue(ok, detail)
            self.assertIn("systemctl suspend", Ag.calls[0][1]["arg"][1])
            ok, detail = W.run_step(vm, W.parse_step("wake"), d, 2)
            self.assertTrue(ok, detail)
            self.assertIn({"execute": "system_wakeup"}, q.sent)
            put(vm.cfg_path, json.dumps(CFG))  # no S3: refused, not hung
            with self.assertRaises(RuntimeError):
                W.run_step(vm, W.parse_step("suspend"), d, 3)

    def test_power_cut(self):
        stages = iter(["", "wolos-install: disk 5", "wolos-install: files 15 Copying"])
        stopped = []
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W.VM, "pid", lambda self: 4242), \
                unittest.mock.patch.object(W, "install_stage", lambda vm: next(stages)), \
                unittest.mock.patch.object(W, "cmd_stop", lambda a: stopped.append(a.force) or 0), \
                unittest.mock.patch.object(W.time, "sleep", lambda s: None):
            ok, detail = W.run_step(self.vm(d), W.parse_step({"power-cut": "files 1[0-9]", "delay": 1}), d, 1)
        self.assertTrue(ok, detail)
        self.assertEqual(detail, "CUT at: wolos-install: files 15 Copying")
        self.assertEqual(stopped, [True])  # forced: no powerdown, nothing synced

    def test_frames_and_heads_in_journey(self):
        shots = []

        def fake_shot(vm, out, nudge=True, head=None, q=None):
            shots.append((os.path.basename(out), nudge, head))
            put(out, b"x")
            return 1280, 800
        clock = iter([0.0, 0.0, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2] + [5.0] * 50)
        with tempfile.TemporaryDirectory() as d, \
                unittest.mock.patch.object(W, "DRY", False), \
                unittest.mock.patch.object(W, "screenshot", fake_shot), \
                unittest.mock.patch.object(W.VM, "qmp", lambda self: FakeQmp()), \
                unittest.mock.patch.object(W.VM, "pid", lambda self: 4242), \
                unittest.mock.patch.object(W.time, "time", lambda: next(clock)), \
                unittest.mock.patch.object(W.time, "sleep", lambda s: None):
            vm = self.vm(d)
            j = os.path.join(d, "j.yaml")
            put(j, "steps:\n- frames: try\n  every: 100\n  for: 0.25\n- note: x\n- sleep: 0\n  heads: all\n")
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = W.cmd_journey(W.argparse.Namespace(home=d, name="t", file=j, out=os.path.join(d, "o"),
                                                        frm=1, keep_going=True))
            self.assertEqual(rc, 0, buf.getvalue())
            recs = [json.loads(x) for x in get(os.path.join(d, "o", "journey.jsonl")).splitlines()][1:]
        frames = [s for s in shots if s[0].startswith("01-try-")]
        self.assertGreaterEqual(len(frames), 2)
        self.assertTrue(all(not nudge for _, nudge, _ in frames))  # a nudge would repaint the frames judged
        self.assertIsNone(recs[0]["shot"])  # no extra screenshot after a burst
        self.assertEqual(recs[0]["more"], [f for f, _, _ in frames])
        self.assertEqual([(f, h) for f, _, h in shots if f.startswith("03-")], [("03-sleep-0-h0.png", 0), ("03-sleep-0-h1.png", 1)])
        self.assertEqual(recs[2]["shot"], "03-sleep-0-h0.png")
        self.assertEqual(recs[2]["more"], ["03-sleep-0-h1.png"])


class NewCli(unittest.TestCase):
    run_cli = DryRun.run_cli
    make_fw = DryRun.make_fw

    def test_new_hardware_flags_dry(self):
        with tempfile.TemporaryDirectory() as d:
            fw, iso = self.make_fw(d)
            os.environ["WOLVM_OVMF"] = fw
            try:
                with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                    self.run_cli("--home", d, "--dry-run", "new", "hw", "--iso", iso, "--tpm", "--no-tpm")
                with contextlib.redirect_stderr(io.StringIO()):
                    rc, out = self.run_cli("--home", d, "--dry-run", "new", "hw", "--iso", iso, "--audio", "2",
                                           "--displays", "2", "--s3", "--disk-bus", "nvme", "--disk-lbs", "4096",
                                           "--extra-bus", "sata", "--drive", iso, "--qemu-arg", "-device usb-kbd",
                                           "--no-tpm")
            finally:
                del os.environ["WOLVM_OVMF"]
            self.assertEqual(rc, 0)
            for want in ("hda-output,bus=hda1.0,audiodev=snd0", "virtio-vga,id=vga0,max_outputs=2",
                         "ICH9-LPC.disable_s3=0", "nvme,drive=disk0,serial=WOLNVME0,logical_block_size=4096",
                         "ide-hd,drive=extra0,bus=ahci0.1", "-device usb-kbd -daemonize", '"tpm": false',
                         '"displays": 2', '"s3": true'):
                self.assertIn(want, out)
            self.assertNotIn("-tpmdev", out)

    def test_bad_counts_refused(self):
        with tempfile.TemporaryDirectory() as d:
            fw, iso = self.make_fw(d)
            os.environ["WOLVM_OVMF"] = fw
            try:
                for argv in (["--audio", "9"], ["--displays", "0"], ["--displays", "2", "--vmsvga"]):
                    with self.assertRaises(SystemExit, msg=argv), contextlib.redirect_stderr(io.StringIO()):
                        self.run_cli("--home", d, "--dry-run", "new", "x", "--iso", iso, *argv)
            finally:
                del os.environ["WOLVM_OVMF"]

    def test_set_hardware_dry(self):
        with tempfile.TemporaryDirectory() as d:
            vm = os.path.join(d, "vms", "t")
            os.makedirs(vm)
            put(os.path.join(vm, "vm.json"), json.dumps(CFG))
            rc, out = self.run_cli("--home", d, "--dry-run", "set", "t", "--audio", "1", "--displays", "3",
                                   "--s3", "on", "--tpm", "on")
            self.assertEqual(rc, 0)
            cfg = json.loads(out)
            self.assertEqual((cfg["audio"], cfg["displays"], cfg["s3"], cfg["tpm"]), (1, 3, True, True))

    def test_qmp_plug_display_dry(self):
        with tempfile.TemporaryDirectory() as d:
            vm = os.path.join(d, "vms", "t")
            os.makedirs(vm)
            put(os.path.join(vm, "vm.json"), json.dumps(dict(CFG, displays=2)))
            rc, out = self.run_cli("--home", d, "--dry-run", "qmp", "t", "system_wakeup")
            self.assertEqual(json.loads(out), {"execute": "system_wakeup"})
            rc, out = self.run_cli("--home", d, "--dry-run", "plug", "t", "usb-audio")
            self.assertIn('"driver": "usb-audio", "id": "usbaudio0"', out)
            rc, out = self.run_cli("--home", d, "--dry-run", "display", "t", "2", "on")
            self.assertIn("Virtual-2/force", out)
            rc, out = self.run_cli("--home", d, "--dry-run", "screenshot", "t", os.path.join(d, "a.png"), "--head", "1")
            self.assertIn('"device": "vga0", "head": 1', out)
            rc, out = self.run_cli("--home", d, "--dry-run", "disk-edit", "t", "--nbd", "/dev/nbd3", "ls $ESP")
            self.assertEqual(rc, 0)
            self.assertIn("qemu-img snapshot -c pre-edit-", out)
            self.assertIn("qemu-nbd --connect /dev/nbd3 --format qcow2", out)
            self.assertIn("qemu-nbd --disconnect /dev/nbd3", out)


class HarnessFixes(unittest.TestCase):
    """IMAGE-013 round-1 harness fixes (a red that was the harness, not the image)."""

    def test_click_accepts_list_and_pair(self):
        for spec in ({"click": "720 590"}, {"click": [720, 590]}, {"click": "[720, 590]"}):
            st = W.parse_step(spec)
            with unittest.mock.patch.object(W, "click") as c:
                ok, _ = W.run_step(None, st, "/tmp", 1)
            self.assertTrue(ok)
            self.assertEqual(c.call_args[0][1:3], (720, 590), spec)

    def _guest_timeout(self, step):
        with unittest.mock.patch.object(W, "guest", side_effect=subprocess.TimeoutExpired("ssh", 5)):
            return W.run_step(None, W.parse_step(step), "/tmp", 1)

    def test_ssh_reboot_with_rc_any_does_not_raise(self):
        ok, detail = self._guest_timeout({"ssh": "systemctl reboot", "rc": "any"})
        self.assertTrue(ok)
        self.assertIn("went down", detail)

    def test_ssh_timeout_still_raises_otherwise(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            self._guest_timeout({"ssh": "systemctl reboot"})  # no rc: any
        with self.assertRaises(subprocess.TimeoutExpired):
            self._guest_timeout({"ssh": "sleep 999", "rc": "any"})  # not a shutdown command

    def _stable(self, frames, clock):
        it = iter(frames)

        class Q:
            def call(self, cmd):
                pass
        t = [0.0]
        vm = unittest.mock.Mock()
        vm.qmp.return_value = Q()
        vm.p.return_value = "/x"
        with unittest.mock.patch.object(W, "nudge"), unittest.mock.patch.object(W, "qmp_screendump"), \
                unittest.mock.patch.object(W, "read_file", lambda p, m: next(it)), \
                unittest.mock.patch.object(W.time, "time", lambda: t[0]), \
                unittest.mock.patch.object(W.time, "sleep", lambda s: t.__setitem__(0, t[0] + clock)):
            return W.wait_stable(vm, 300), t[0]

    def test_black_frame_not_stable_before_90s(self):
        hdr = b"P6\n1280 800\n255\n"
        black, lit = hdr + b"\0" * 64, hdr + b"\x10" * 64
        # steady black: stable only once 90 s have passed
        ok, t = self._stable([black] * 60, 10.0)
        self.assertTrue(ok)
        self.assertGreaterEqual(t, 90)
        # steady non-black: stable on the second frame
        ok, t = self._stable([lit] * 5, 2.0)
        self.assertTrue(ok)
        self.assertLess(t, 10)

    def test_journey_fixes_in_place(self):
        J = os.path.join(HERE, "journeys")
        sb = get(os.path.join(J, "qa100-boot-live-sb-off-later.yaml"))
        self.assertNotIn("grep -c 'CN=", sb)  # a self-signed key names the CN twice
        self.assertIn("key-enrolled", sb)
        gui = [s["verb"] for s in W.load_journey(os.path.join(J, "qa100-install-gui-erase.yaml"))[1]]
        self.assertLess(gui.index("wait-text"), gui.index("checkpoint"))  # Welcome card seen before begin
        live = ["qa100-net-ethernet-states", "qa100-net-sharing-live", "qa100-net-static-ip",
                "qa100-net2-two-nics-portal", "qa100-net3-airplane", "qa100-net3-proxy-dns", "qa100-net3-vpn",
                "qa100-wm-decorations-xwayland-fill", "qa100-wm-minimise-focus", "qa100-wm-semaphore-states"]
        for j in live:
            text = get(os.path.join(J, j + ".yaml"))
            self.assertIn("- sleep: 30\n", text, j)
            self.assertIn("Known wait (30 s)", text, j)  # a wait is explained, not silent

    def test_wv_wrapper_starts_vm_unless_first_step_is_start(self):
        wv = os.path.join(HERE, "wv")
        self.assertTrue(os.access(wv, os.X_OK))
        text = get(wv)
        self.assertIn('"- start"*) ;;', text)
        self.assertIn("stop \"$2\" --force", text)


class AllJourneys(unittest.TestCase):
    """Every journey in tests/wolvm/journeys parses and dry-runs (wolvm --dry-run journey), and every
    qa-/qa100- journey has exactly one line in the qa100 gate manifest."""

    def test_dry_run_every_journey(self):
        files = sorted(f for f in os.listdir(os.path.join(HERE, "journeys")) if f.endswith((".yaml", ".json")))
        self.assertGreater(len(files), 70)
        for f in files:
            _, steps = W.load_journey(os.path.join(HERE, "journeys", f))
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = W.main(["--dry-run", "journey", "x", os.path.join(HERE, "journeys", f)])
            lines = buf.getvalue().splitlines()
            self.assertEqual(rc, 0, f)
            self.assertEqual(len(lines), len(steps), f)
            self.assertTrue(all(ln[:2].isdigit() for ln in lines), f)

    def test_manifest_covers_every_qa_journey(self):
        rows = {}
        for ln in get(os.path.join(HERE, "qa100.manifest")).splitlines():
            if not ln.strip() or ln.lstrip().startswith("#"):
                continue
            cols = [c.strip() for c in ln.split("|")]
            self.assertEqual(len(cols), 4, ln)
            j, needs, expect, why = cols
            self.assertNotIn(j, rows, f"{j} twice")
            rows[j] = (needs, expect)
            self.assertRegex(expect, r"^(pass|fail:(QA-[A-Z0-9]+(-[A-Z0-9]+)?-\d+[a-z]?|[a-z0-9-]+:[A-Z0-9]+-\d+)"
                                     r"(@(visual|\d+(,\d+)*))?)$", ln)
            self.assertTrue(why, ln)
            base = [n for n in needs.split(",") if n in ("live", "installed", "iso-install")]
            self.assertEqual(len(base), 1, f"{j}: exactly one of live/installed/iso-install")
            for n in needs.split(","):
                self.assertRegex(n, r"^(live|installed|iso-install|network|audio|no-audio|s3|usb|disk-edit|tpm|"
                                    r"secureboot|drive|ssh|release-0120|displays:[2-4]|account:[a-z0-9]+)$", f"{j}: {n}")
        qa = sorted(f[:-5] for f in os.listdir(os.path.join(HERE, "journeys")) if f.startswith(("qa-", "qa100-")))
        self.assertEqual(sorted(rows), qa)
        # a pinned step exists and really asserts (a wait, or an expect): a step that cannot fail
        # would turn every run into INFRA
        for j, (_, expect) in rows.items():
            if "@" not in expect or expect.endswith("@visual"):
                continue
            steps = W.load_journey(os.path.join(HERE, "journeys", j + ".yaml"))[1]
            for n in expect.split("@")[1].split(","):
                self.assertLessEqual(int(n), len(steps), f"{j}: no step {n}")
                st = steps[int(n) - 1]
                self.assertTrue(st["verb"] in ("wait-agent", "wait-text", "wait-stable", "power-cut") or "expect" in st["opts"],
                                f"{j} step {n} ({st['verb']}) asserts nothing")
        # the hardware a journey's own header asks for is in its needs
        for j, (needs, _) in rows.items():
            text = get(os.path.join(HERE, "journeys", j + ".yaml"))
            verbs = {s["verb"] for s in W.load_journey(os.path.join(HERE, "journeys", j + ".yaml"))[1]}
            if verbs & {"suspend", "wake"}:
                self.assertIn("s3", needs, j)
            if verbs & {"plug", "unplug"}:
                self.assertIn("usb", needs, j)
            if "Virtual-2" in text:
                self.assertIn("displays:", needs, j)
            if "qemu-nbd" in text or "disk-edit" in verbs:
                self.assertIn("disk-edit", needs, j)


@unittest.skipUnless(shutil.which("sh") and shutil.which("python3"), "needs a POSIX sh")
class Gate(unittest.TestCase):
    """run-installed.sh qa100 on a stub wolvm: each journey's journey.jsonl decides the result."""
    STUB = """#!/bin/sh
case "$1" in
  status) exit 0 ;;
  journey) f=$(basename "$3" .yaml); out=$6; mkdir -p "$out"
    printf '{"journey": "%s"}\\n' "$f" > "$out/journey.jsonl"
    case $f in
      ok|fixed|vis-ok) printf '{"n": 1, "step": "ssh a", "ok": true}\\n' >> "$out/journey.jsonl"; exit 0 ;;
      at3) printf '{"n": 2, "step": "ssh a", "ok": true}\\n{"n": 3, "step": "ssh bug", "ok": false}\\n' >> "$out/journey.jsonl" ;;
      *) printf '{"n": 1, "step": "wait-agent", "ok": false}\\n{"n": 3, "step": "ssh bug", "ok": false}\\n' >> "$out/journey.jsonl" ;;
    esac; exit 1 ;;
esac
exit 2
"""

    def test_results(self):
        with tempfile.TemporaryDirectory() as d:
            stub = os.path.join(d, "wolvm")
            put(stub, self.STUB)
            os.chmod(stub, 0o755)
            man = os.path.join(d, "m")
            put(man, "# x\nok | live | pass | a\nbroke | live | pass | b\nat3 | live | fail:QA-X-1@3,5 | c\n"
                     "early | live | fail:QA-X-2@3 | d\nfixed | live | fail:QA-X-3@1 | e\nloose | live | fail:QA-X-4 | f\n"
                     "vis-ok | live | fail:QA-X-5@visual | g\nvis | live | fail:QA-X-6@visual | h\nfar | live,s3 | pass | i\n")
            env = dict(os.environ, WOLOS_WOLVM=stub, QA100_MANIFEST=man)
            gate = os.path.join(os.path.dirname(HERE), "run-installed.sh")
            r = subprocess.run(["sh", gate, "qa100", "--vm", "v", "--have", "live", "--out", os.path.join(d, "o")],
                               env=env, text=True, capture_output=True)
            res = {ln.split("\t")[0]: ln.split("\t") for ln in get(os.path.join(d, "o", "qa100-summary.tsv")).splitlines()[1:]}
            self.assertEqual({k: v[1] for k, v in res.items()},
                             {"ok": "PASS", "broke": "FAIL", "at3": "XFAIL", "early": "INFRA", "fixed": "XPASS",
                              "loose": "XFAIL", "vis-ok": "REVIEW", "vis": "REVIEW", "far": "SKIP"}, r.stdout)
            self.assertEqual(res["early"][5], "1 wait-agent")
            self.assertIn("first failure: step 1 wait-agent; expected at 3", r.stdout)
            self.assertIn("1 PASS, 2 XFAIL, 1 FAIL, 1 XPASS, 2 REVIEW, 1 SKIP; INFRA 1", r.stdout)
            self.assertEqual(r.returncode, 1)  # the FAIL
            put(man, "early | live | fail:QA-X-2@3 | d\nat3 | live | fail:QA-X-1@3 | c\n")
            r = subprocess.run(["sh", gate, "qa100", "--vm", "v", "--have", "live", "--out", os.path.join(d, "o2")],
                               env=env, text=True, capture_output=True)
            self.assertEqual(r.returncode, 0, r.stdout)  # INFRA alone is not fatal...
            r = subprocess.run(["sh", gate, "qa100", "--vm", "v", "--have", "live", "--strict", "--out", os.path.join(d, "o3")],
                               env=env, text=True, capture_output=True)
            self.assertEqual(r.returncode, 1, r.stdout)  # ...but is with --strict


if __name__ == "__main__":
    unittest.main()
