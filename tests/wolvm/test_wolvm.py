#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Unit tests for wolvm: step parser, key/QMP builders, qemu command, PNG writer, report.
Run: python3 tests/wolvm/test_wolvm.py   (no QEMU needed; also runs on Windows)."""
import contextlib, io, json, os, struct, subprocess, sys, tempfile, unittest, zlib
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


class PutStep(unittest.TestCase):
    """The put verb sends a host file into the guest through the agent (idle.py for desktop-idle.yaml)."""
    def test_parse_and_send(self):
        _, steps = W.load_journey(os.path.join(HERE, "journeys", "desktop-idle.yaml"))
        st = next(s for s in steps if s["verb"] == "put")
        self.assertEqual(st["args"], ["/tmp/idle.py"])
        self.assertEqual(st["opts"]["file"], "design/tools/perf/idle.py")
        with tempfile.TemporaryDirectory() as d:
            put(os.path.join(d, "x.py"), "print(1)\n")
            sent = []

            class A:
                def put(self, path, data, mode):
                    sent.append((path, data, mode))

            class V:
                def agent(self):
                    return A()

            ok, detail = W.run_step(V(), {"verb": "put", "args": ["/tmp/x.py"],
                                          "opts": {"file": os.path.join(d, "x.py"), "mode": "0755"}}, d, 1)
        self.assertTrue(ok, detail)
        self.assertEqual(sent, [("/tmp/x.py", b"print(1)\n", "0755")])

    def test_relative_file_is_looked_up_under_wolos_src(self):
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(os.path.join(d, "a"))
            put(os.path.join(d, "a", "b.py"), "")
            old = os.environ.get("WOLOS_SRC")
            os.environ["WOLOS_SRC"] = d
            try:
                self.assertEqual(W.host_file("a/b.py"), os.path.join(d, "a", "b.py"))
                with self.assertRaises(RuntimeError):
                    W.host_file("a/missing.py")
            finally:
                if old is None:
                    del os.environ["WOLOS_SRC"]
                else:
                    os.environ["WOLOS_SRC"] = old


if __name__ == "__main__":
    unittest.main()
