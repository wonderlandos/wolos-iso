// SPDX-License-Identifier: GPL-2.0-or-later
//! VM scripts on a Linux host (ADR 0016): QEMU/KVM with OVMF and virtio-gpu (GL), so Hyprland runs
//! in the VM. build.sh fails fast short of disk; test-install.sh installs unattended from the ISO
//! (wolos.autoinstall) next to a bystander disk that must come out byte-identical, from a path or
//! from release artefacts (`--release DIR`, hash checked), on KVM or, without it, on TCG. The
//! install tests drive their VMs through wolvm (tests/wolvm/wolvm), not their own qemu lines.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn scripts_parse_and_freebsd_vm_tooling_is_gone() {
    for s in [
        "tests/run.sh",
        "tests/run-installed.sh",
        "tests/test-install.sh",
        "tests/run-gpu.sh",
        "tests/test-audio.sh",
    ] {
        let o = Command::new("sh").arg("-n").arg(repo(s)).output().unwrap();
        assert!(
            o.status.success(),
            "{s}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let mode = fs::metadata(repo(s)).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "{s} executable");
        let t = read(s);
        for gone in ["ssh.sh", "build-in-vm", "bsdinstall", "zpool", "memstick"] {
            assert!(!t.contains(gone), "{s} still uses {gone}");
        }
    }
    for gone in [
        // One build path: wolos-build iso / build.sh (audit BF-9).
        "tests/build.sh",
        "tests/build-in-vm.sh",
        "tests/ssh.sh",
        "tests/update-disk.sh",
        "tests/up.sh",
    ] {
        assert!(!repo(gone).exists(), "{gone}");
    }
}

/// A full disk paused a build mid-way once and the build died silently: refuse up front.
#[test]
fn build_refuses_to_start_short_of_disk() {
    let d = std::env::temp_dir().join(format!("wol-vmbuild-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("bin")).unwrap();
    for (name, body) in [
        (
            "df",
            "#!/bin/sh\necho 'Filesystem 1M-blocks Used Available Use% Mounted'\necho '/dev/x 400000 395000 5000 99% /home'\n",
        ),
        (
            "mkarchiso",
            "#!/bin/sh\necho mkarchiso-called >&2\nexit 9\n",
        ),
    ] {
        let p = d.join("bin").join(name);
        fs::write(&p, body).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = Command::new("bash")
        .arg(repo("build.sh"))
        .env("PATH", format!("{}:/usr/bin:/bin", d.join("bin").display()))
        .env("WOLOS_VM", &d)
        .env_remove("WOLOS_WORK")
        .env_remove("WOLOS_STAGE_ONLY")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("host has 5000 MB free"), "{err}");
    assert!(!err.contains("mkarchiso-called"), "{err}");
}

#[test]
fn vms_use_ovmf_kvm_and_virtio_gpu() {
    for s in ["tests/run.sh", "tests/run-installed.sh"] {
        let t = read(s);
        for want in [
            "qemu-system-x86_64",
            "-enable-kvm",
            "OVMF_CODE.4m.fd",
            "OVMF_VARS.4m.fd",
            "${WOLOS_VM:-$HOME/WolOS-vm}",
        ] {
            assert!(t.contains(want), "{s} lacks {want}");
        }
        // Interactive VMs draw with GL.
        assert!(t.contains("virtio-vga-gl"), "{s} lacks virtio-vga-gl");
    }
}

/// The unattended tests are thin wrappers over wolvm: it owns qemu, OVMF, swtpm and KVM-or-TCG;
/// they keep their CLI and checks. Offline (--no-net), the ISO kernel booted with our words
/// (--direct), the 2D virtio-vga (QMP screendumps cannot read a GL scanout), extra raw disks.
#[test]
fn install_tests_drive_their_vms_through_wolvm() {
    for s in ["tests/test-install.sh", "tests/test-installer.sh"] {
        let t = read(s);
        assert!(
            t.contains(r#""$REPO/tests/wolvm/wolvm" --home"#)
                || (t.contains(r#"WOLVM=${WOLOS_WOLVM:-$REPO/tests/wolvm/wolvm}"#)
                    && t.contains(r#""$WOLVM" --home"#)),
            "{s} does not use wolvm"
        );
        let own = t
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| l.contains("qemu-system-x86_64 ") && !l.contains("command -v"));
        assert!(!own, "{s} still runs qemu itself");
        assert!(
            !t.contains("qmp.py"),
            "{s}: screenshots and keys go through wolvm"
        );
        for want in [
            "--direct",
            "--no-net",
            "--gpu virtio",
            "--from-image",
            "--force",
        ] {
            if want == "--from-image" && s.ends_with("test-install.sh") {
                continue;
            }
            assert!(t.contains(want), "{s} lacks {want}");
        }
    }
    let t = read("tests/test-install.sh");
    for want in [
        r#"--drive "$T/other.raw""#,
        r#"opts="--drive $T/keys.raw""#,
        r#"opts="--tpm""#,
        r#"opts="$opts --secureboot""#,
        "sb_enforce off",
        "sb_enforce on",
        "--boot disk --no-iso",
        r#"qemu-img convert -f qcow2 -O raw "$T/wolvm/vms/$N/disk.qcow2" install.raw"#,
    ] {
        assert!(t.contains(want), "test-install.sh lacks {want}");
    }
    let i = read("tests/test-installer.sh");
    assert!(i.contains("--iso-bus virtio"), "medium: the ISO as a disk");
    assert!(i.contains(r#"[ "$2" != tpm ] || { tpm=--tpm; net=; }"#));
}

#[test]
fn install_test_is_unattended_and_proves_the_bystander_disk_untouched() {
    let t = read("tests/test-install.sh");
    for want in [
        "wolos.autoinstall=/dev/vda",
        "--direct",
        "other.raw",
        "BOOT PASS",
        "wolos-install: done",
        "sfdisk -d",
        "WOLOS_KEEP_DISK",
        "$T/disk.raw",
        // Booted means the firmware started the disk's boot manager and the screen shows a UI,
        // not just any screenshot (firmware's "no bootable device" screen is a screenshot too).
        "Linux Boot Manager",
        "getcolors",
        // One run per $WOLOS_VM home (its pidfile), not "no QEMU at all on the host".
        "lock=$T/test-install.pid",
    ] {
        assert!(t.contains(want), "test-install.sh lacks {want}");
    }
    assert!(
        !t.contains("pgrep -x qemu-system-x86"),
        "test-install.sh must not refuse to run beside other harnesses' or people's VMs"
    );
}

/// Prerelease run 2026-11-03 (B10): the guard is this harness's own pidfile in $WOLOS_VM/test.
/// A live run in the same home stops a second one; a stale pidfile does not; any other QEMU on
/// the host (a pgrep that finds one) is not consulted at all.
#[test]
fn install_test_guards_its_own_home_not_every_qemu() {
    let d = std::env::temp_dir().join(format!("wol-vmguard-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let bin = d.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for (name, body) in [
        ("qemu-system-x86_64", "exit 0"),
        ("qemu-img", "exit 0"),
        ("mdir", "exit 0"),
        // A foreign VM is running: the old guard would have stopped here.
        ("pgrep", "echo 4242; exit 0"),
    ] {
        let p = bin.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let t = d.join("vm").join("test");
    fs::create_dir_all(&t).unwrap();
    let run = || {
        Command::new("sh")
            .arg(repo("tests/test-install.sh"))
            .arg(d.join("missing.iso"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("WOLOS_VM", d.join("vm"))
            .output()
            .unwrap()
    };
    // Another run (this test process, alive) owns the home.
    fs::write(
        t.join("test-install.pid"),
        format!("{}\n", std::process::id()),
    )
    .unwrap();
    let err = String::from_utf8_lossy(&run().stderr).into_owned();
    assert!(err.contains("another test-install run"), "{err}");
    // Its owner is gone (a pid that cannot be alive): the run goes on, past the guard, to the ISO.
    fs::write(t.join("test-install.pid"), "999999999\n").unwrap();
    let o = run();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(!err.contains("another"), "{err}");
    assert!(err.contains("no ISO in"), "{err}");
    assert!(
        !t.join("test-install.pid").exists(),
        "the run removes its pidfile on exit"
    );
    let _ = fs::remove_dir_all(&d);
}

/// `layouts` has a verdict like `run`: its output goes to $W/layouts.result and verdict.sh decides,
/// so a FAIL, or a run where every name was skipped (no PASS), exits non-zero. The run itself on
/// skip-only names (no sudo, no disk needed): exit 1 with the clear line.
#[test]
fn layouts_has_a_verdict_and_fails_when_everything_skipped() {
    let i = read("tests/test-installer.sh");
    assert!(i.contains(r#"layouts "$@" 2>&1 | tee "$W/layouts.result""#));
    assert!(i.contains(r#"sh "$REPO/tests/test/verdict.sh" E2E "$W/layouts.result""#));
    let d = std::env::temp_dir().join(format!("wol-layouts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(repo("tests/test-installer.sh"))
            .arg("layouts")
            .args(args)
            .env("WOLOS_E2E", &d)
            .output()
            .unwrap()
    };
    let o = run(&["/nonexistent", "erase", "password", "tpm"]);
    let (out, err) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    assert!(!o.status.success(), "{out}{err}");
    assert_eq!(out.matches("E2E SKIP").count(), 3, "{out}");
    assert!(err.contains("every name was skipped"), "{err}");
    assert!(
        out.contains("1 of 1 result files not clean"),
        "verdict.sh decided: {out}"
    );
    // No names at all: a usage error, not a pass.
    let o = run(&["/nonexistent"]);
    assert_eq!(o.status.code(), Some(2));
    let _ = fs::remove_dir_all(&d);
}

/// Prerelease run 2026-11-03: the harness bugs it found stay fixed. B2 the live journey waits for
/// the live system's own first unit, not a greeting this console never prints; B4 the disk comes
/// back from the VM with a bounded wait for the qcow2 lock; B4b `layouts` skips the whole-disk
/// erases (systemd-repart) and makes the throw-away key an encrypted plan formats with; B5 the
/// MOK screens are driven by the text the firmware mirrors to the serial console; B6 the tpm
/// variants have a network card.
#[test]
fn prerelease_harness_fixes_stay() {
    let j = read("tests/wolvm/journeys/live-boot.yaml");
    assert!(
        j.contains(r#"wait-text: "Install WolOS: the first screen of the live image""#),
        "{j}"
    );
    assert!(!j.contains(r#"wait-text: "Welcome to""#));
    let i = read("tests/test-installer.sh");
    let to_raw = i
        .split("to_raw() {")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    for want in [
        "for i in 1 2 3 4 5 6 7 8 9 10 11 12; do",
        "grep -qi 'lock'",
        "E2E FAIL",
        "return 1",
    ] {
        assert!(to_raw.contains(want), "to_raw lacks {want}");
    }
    let layouts = i
        .split("layouts() {")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(layouts.contains("erase | password | tpm) echo \"E2E SKIP $S:"));
    assert!(layouts.contains(r#"head -c 64 /dev/urandom > "$d/key""#));
    assert!(layouts.contains(r#"encrypted-alongside) mk_disk windows "$d/disk.raw" ;;"#));
    assert!(layouts.contains(r#"sudo cryptsetup close "$m""#));
    assert!(i.contains(r#"[ "$2" != tpm ] || { tpm=--tpm; net=; }"#));
    assert!(i.contains("--direct $net"));
    let t = read("tests/test-install.sh");
    for want in [
        r#"net=--no-net; [ "$ENC" != tpm ] || net="#,
        "--direct $net",
        r#"MOK_MENU="down ret down ret down ret b o o t ret ret""#,
        r#"w wait-text "$N" 'Perform MOK management|Press any key to perform MOK management' --regex"#,
        r#"if [ -n "${WOLOS_MOK_KEYS:-}" ]; then"#,
    ] {
        assert!(t.contains(want), "test-install.sh lacks {want}");
    }
    assert!(
        !t.contains("WOLOS_MOK_KEYS:-ret down"),
        "the old key list starts with Continue boot"
    );
}

/// test-audio.sh runs inside a booted WolOS and proves the sound rules end to end: the WolOS
/// fragments are what PipeWire loaded (64-frame floor, 96 kHz allowed), a 64-frame stream on a
/// null sink plays with 0 xruns, a forced buffer applies and reverts, echo cancel and the App
/// Mix load on demand and unload, `wol audio reset` is a dry run, `wol audio doctor` passes.
#[test]
fn audio_test_proves_floor_force_modules_and_reset_in_the_guest() {
    let t = read("tests/test-audio.sh");
    for want in [
        "pw-metadata -n settings 0",
        "clock.min-quantum",
        "clock.allowed-rates",
        "module-null-sink",
        "sink_name=wolos_test",
        "pw-cat",
        "--latency 64",
        "pw-config -n pipewire.conf merge context.properties",
        "cpu.vm.name",
        "FLOOR=1024",
        "no user session",
        "--raw",
        "pw-top -b -n",
        "clock.force-quantum 256",
        "clock.force-quantum 0",
        "module-echo-cancel",
        "wol audio appmix on",
        "wol audio appmix off",
        "wol audio reset",
        "wpctl clear-default",
        "wol audio doctor",
        "trap cleanup EXIT",
        "test-audio: PASS",
    ] {
        assert!(t.contains(want), "test-audio.sh lacks {want}");
    }
    assert!(
        !t.contains("reset --yes"),
        "the guest test never resets for real"
    );
}

/// `--release DIR` (wolos-build release): DIR's one ISO must match the sha256 of its manifest
/// (release.json, else build.sh's <iso>.json) and its <iso>.sha256 before any VM starts. Fakes:
/// qemu, mdir, pgrep (no other VM) and an OVMF dir; the fake ISO then fails at the kernel step,
/// which proves the check passed.
#[test]
fn install_test_runs_release_artefacts_only_when_their_hash_matches() {
    let d = std::env::temp_dir().join(format!("wol-vmrelease-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let (bin, ovmf, rel) = (d.join("bin"), d.join("ovmf"), d.join("rel"));
    for p in [&bin, &ovmf, &rel] {
        fs::create_dir_all(p).unwrap();
    }
    for name in ["qemu-system-x86_64", "mdir", "pgrep"] {
        let p = bin.join(name);
        fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(ovmf.join("OVMF_CODE.4m.fd"), "").unwrap();
    fs::write(ovmf.join("OVMF_VARS.4m.fd"), "").unwrap();
    let iso = rel.join("wolos-0.12.0-x86_64.iso");
    fs::write(&iso, "iso bytes").unwrap();
    // sha256("iso bytes")
    let sum = Command::new("sha256sum").arg(&iso).output().unwrap();
    let sum = String::from_utf8_lossy(&sum.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    let run = || {
        Command::new("sh")
            .arg(repo("tests/test-install.sh"))
            .arg("--release")
            .arg(&rel)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("WOLOS_VM", d.join("vm"))
            .env("WOLOS_OVMF", &ovmf)
            .output()
            .unwrap()
    };
    // build.sh's manifest, the form release-manifest.sh prints.
    let man = rel.join("wolos-0.12.0-x86_64.json");
    fs::write(
        &man,
        format!("{{\"version\":\"0.12.0\",\"sha256\":\"{sum}\",\"size\":9}}\n"),
    )
    .unwrap();
    let o = run();
    let (out, err) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    assert!(out.contains(&format!("({sum})")), "{out}{err}");
    assert!(
        err.contains("no kernel in"),
        "the check passed, the fake ISO stops at the kernel: {err}"
    );
    // release.json (json.dump spacing) wins over <iso>.json; a wrong hash stops before the VM.
    fs::write(
        rel.join("release.json"),
        format!("{{\"sha256\": \"{}\"}}\n", "0".repeat(64)),
    )
    .unwrap();
    let o = run();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        !o.status.success() && err.contains("not the released image"),
        "{err}"
    );
    assert!(!err.contains("no kernel in"), "{err}");
    fs::write(
        rel.join("release.json"),
        format!("{{\"sha256\": \"{sum}\"}}\n"),
    )
    .unwrap();
    fs::write(
        rel.join("wolos-0.12.0-x86_64.iso.sha256"),
        format!("{} wolos-0.12.0-x86_64.iso\n", "1".repeat(64)),
    )
    .unwrap();
    let err = String::from_utf8_lossy(&run().stderr).into_owned();
    assert!(err.contains(".sha256 does not match"), "{err}");
    // Two ISOs: which one was released is not guessed.
    fs::remove_file(rel.join("wolos-0.12.0-x86_64.iso.sha256")).unwrap();
    fs::write(rel.join("wolos-0.11.0-x86_64.iso"), "old").unwrap();
    let err = String::from_utf8_lossy(&run().stderr).into_owned();
    assert!(err.contains("exactly one wolos-*.iso"), "{err}");
}

/// No /dev/kvm (a laptop VM, a runner): wolvm runs the VM on TCG; the install test says so and
/// lengthens its timeouts.
#[test]
fn install_test_falls_back_to_tcg_with_longer_timeouts() {
    let t = read("tests/test-install.sh");
    for want in [
        "[ -r /dev/kvm ] && [ -w /dev/kvm ]",
        "WOLOS_TCG_SCALE",
        "running on TCG",
        "$((1800 * scale))",
        "$((240 * scale))",
    ] {
        assert!(t.contains(want), "test-install.sh lacks {want}");
    }
}

/// audit5 I6-2: the VM tests' exit status follows their result lines (tests/test/verdict.sh):
/// one FAIL, or a result file without any PASS (a scenario that never got to its checks), is 1.
#[test]
fn vm_tests_exit_non_zero_when_a_check_fails() {
    let v = repo("tests/test/verdict.sh");
    let d = std::env::temp_dir().join(format!("wol-verdict-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let run = |files: &[(&str, &str)]| {
        let mut args = vec!["BOOT".to_string()];
        for (n, body) in files {
            let p = d.join(n);
            if !body.is_empty() {
                fs::write(&p, body).unwrap();
            }
            args.push(p.display().to_string());
        }
        Command::new("sh")
            .arg(&v)
            .args(&args)
            .status()
            .unwrap()
            .code()
    };
    assert_eq!(run(&[("ok.txt", "BOOT PASS a\nBOOT PASS b\n")]), Some(0));
    assert_eq!(run(&[("one.txt", "BOOT PASS a\nBOOT FAIL b\n")]), Some(1));
    assert_eq!(run(&[("none.txt", "wolos-install: failed\n")]), Some(1));
    assert_eq!(
        run(&[("ok.txt", "BOOT PASS a\n"), ("missing.txt", "")]),
        Some(1)
    );
    let _ = fs::remove_dir_all(&d);
    let t = read("tests/test-install.sh");
    assert!(
        t.trim_end()
            .ends_with(r#"sh "$REPO/tests/test/verdict.sh" BOOT checks.txt"#),
        "test-install.sh: the verdict is its last command"
    );
    let i = read("tests/test-installer.sh");
    assert!(i.contains(r#"sh "$REPO/tests/test/verdict.sh" E2E $res"#));
}

/// audit5 I6-3: mok fails when enforcement could not be turned on, and asserts Secure Boot is on
/// in the booted system.
#[test]
fn mok_tier_proves_secure_boot_is_on() {
    let t = read("tests/test-install.sh");
    assert!(t.contains(r#"w ssh "$N" "mokutil --sb-state" > sb-state.txt"#));
    assert!(t.contains(
        r#"b "mok tier: Secure Boot enabled in the booted system" "grep -q 'SecureBoot enabled' sb-state.txt""#
    ));
    // The script itself on a stub wolvm (WOLOS_WOLVM) whose `set --secureboot <state>` fails, and
    // no qemu-img for the fallback: the run stops with exit 1 before any boot of the disk.
    let d = std::env::temp_dir().join(format!("wol-mok-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let bin = d.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let stub = |name: &str, body: &str| {
        let p = bin.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    };
    for ok in ["qemu-system-x86_64", "mdir", "virt-fw-vars", "bsdtar"] {
        stub(ok, "exit 0");
    }
    stub("qemu-img", "exit 1");
    stub("pgrep", "exit 1");
    stub("sha256sum", r#"echo "0  $1""#);
    stub(
        "wolvm",
        r#"echo "$*" >> "$LOG"
case "$*" in
  *"set install --secureboot $FAIL_ON") exit 1 ;;
  *"status install") echo '{"running": false}' ;;
esac
exit 0"#,
    );
    let iso = d.join("wolos-test.iso");
    fs::write(&iso, "iso").unwrap();
    let log = d.join("wolvm.log");
    let run = |fail_on: &str| {
        let _ = fs::remove_file(&log);
        let o = Command::new("sh")
            .arg(repo("tests/test-install.sh"))
            .arg(&iso)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("WOLOS_VM", d.join("vm"))
            .env("WOLOS_WOLVM", bin.join("wolvm"))
            .env("WOLOS_SECBOOT", "mok")
            .env("LOG", &log)
            .env("FAIL_ON", fail_on)
            .output()
            .unwrap();
        let calls = fs::read_to_string(&log).unwrap_or_default();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            calls,
        )
    };
    let (rc, err, calls) = run("off");
    assert_eq!(rc, Some(1), "{err}");
    assert!(
        err.contains("could not turn Secure Boot enforcement off"),
        "{err}"
    );
    assert!(
        calls.contains("new install") && calls.contains("--secureboot"),
        "{calls}"
    );
    assert!(
        !calls.contains("start install"),
        "the install started: {calls}"
    );
    let (rc, err, calls) = run("on");
    assert_eq!(rc, Some(1), "{err}");
    assert!(
        err.contains("could not turn Secure Boot enforcement on"),
        "{err}"
    );
    assert_eq!(
        calls.matches("start install").count(),
        1,
        "the disk booted with Secure Boot off: {calls}"
    );
    assert!(
        calls.contains("set install --boot disk --no-iso"),
        "{calls}"
    );
    let _ = fs::remove_dir_all(&d);
}

/// audit6 I6-1/I6-4: an encrypted disk is asked whether any unit failed (a passphrase disk used to
/// run the chip's reseal on every start), and a tpm or mok disk is started a SECOND time without
/// anyone typing: no unlock prompt on the console, the re-bound policy opens it. The script itself
/// on a stub wolvm whose guest answers from a file, a good and a bad guest and serial log.
#[test]
fn tpm_and_mok_disks_boot_twice_and_encrypted_ones_have_no_failed_unit() {
    let t = read("tests/test-install.sh");
    for want in [
        "guest_facts facts-boot1.txt",
        "guest_facts facts-boot2.txt",
        "wolos-tpm-reseal",
        "w serial \"$N\" tail -n 1000000 > serial-boot2.log",
        r#"b "no failed unit after the first boot ($ENC)""#,
        r#"b "second boot: no unlock prompt on the console""#,
        r#"b "first boot: wolos-tpm-bind ran and finished""#,
    ] {
        assert!(t.contains(want), "test-install.sh lacks {want}");
    }
    let d = std::env::temp_dir().join(format!("wol-twice-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    let bin = d.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let stub = |name: &str, body: &str| {
        let p = bin.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    };
    for ok in [
        "qemu-system-x86_64",
        "mdir",
        "virt-fw-vars",
        "bsdtar",
        "swtpm",
        "mkfs.vfat",
        "mcopy",
        "mmd",
    ] {
        stub(ok, "exit 0");
    }
    stub("qemu-img", "exit 1");
    stub("pgrep", "exit 1");
    stub("sha256sum", r#"echo "0  $1""#);
    // install_scenario <scenario> <probe.json> <plan dir>: a plan for the blank disk it is probed on
    stub(
        "install_scenario",
        r#"echo "$*" >> "$LOG"; grep -q '"/dev/vda"' "$2" && mkdir -p "$3" && echo '{}' > "$3/plan.json""#,
    );
    stub(
        "wolvm",
        r#"echo "$*" >> "$LOG"
case "$*" in
  *"status install") echo '{"running": false}' ;;
  *"== state"*) cat "$FACTS" ;;
  *mokutil*) echo "SecureBoot enabled" ;;
  *"serial install tail"*) cat "$SERIAL" ;;
esac
exit 0"#,
    );
    let iso = d.join("wolos-test.iso");
    fs::write(&iso, "iso").unwrap();
    let good = "== state\nrunning\n== failed\n== bind\nConditionResult=yes\nResult=success\n\
rebind-key-gone\nFinished Bind the disk key to this computer's firmware state (first boot).\n\
== reseal\nstart-rc=0\nResult=success\n== account\n1\n== end\n";
    let bad = good.replace(
        "== failed\n",
        "== failed\nwolos-tpm-reseal.service loaded failed failed Let the chip\n",
    );
    let run_p = |enc: &str, sb: &str, facts: &str, serial: &str, plan: &str| {
        let log = d.join("wolvm.log");
        let _ = fs::remove_file(&log);
        fs::write(d.join("facts"), facts).unwrap();
        fs::write(d.join("serial"), serial).unwrap();
        let _ = fs::remove_dir_all(d.join("vm"));
        let o = Command::new("sh")
            .arg(repo("tests/test-install.sh"))
            .arg(&iso)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("WOLOS_VM", d.join("vm"))
            .env("WOLOS_WOLVM", bin.join("wolvm"))
            .env("WOLOS_ENCRYPT", enc)
            .env("WOLOS_SECBOOT", sb)
            .env("WOLOS_BOOT_S", "1")
            .env("WOLOS_TCG_SCALE", "1")
            .env("LOG", &log)
            .env("FACTS", d.join("facts"))
            .env("SERIAL", d.join("serial"))
            .env("WOLOS_PLAN", plan)
            .env("WOLOS_INSTALL_SCENARIO", bin.join("install_scenario"))
            .output()
            .unwrap();
        let calls = fs::read_to_string(&log).unwrap_or_default();
        (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            calls.matches("start install").count(),
            calls,
        )
    };
    let run = |enc: &str, sb: &str, facts: &str, serial: &str| {
        let (out, err, starts, _) = run_p(enc, sb, facts, serial, "no");
        (out, err, starts)
    };
    let (out, err, starts) = run("tpm", "0", good, "login:\n");
    assert_eq!(starts, 3, "install, boot, second boot: {err}\n{out}");
    for want in [
        "BOOT PASS no failed unit after the first boot (tpm)",
        "BOOT PASS wolos-tpm-reseal does not fail (tpm)",
        "BOOT PASS first boot: wolos-tpm-bind ran and finished",
        "BOOT PASS first boot: the install key is gone after the bind",
        "BOOT PASS second boot: the guest answered without anyone typing",
        "BOOT PASS second boot: no unlock prompt on the console",
        "BOOT PASS second boot: no failed unit",
        "BOOT PASS second boot: wolos-tpm-reseal ran clean",
        "BOOT PASS second boot: the chip unlocked the disk (install key gone)",
    ] {
        assert!(out.contains(want), "{want}\n{out}\n{err}");
    }
    // The chip asked for the password on the second start, a unit failed: both are caught.
    let (out, _, _) = run(
        "tpm",
        "0",
        &bad,
        "Please enter passphrase for disk wolosroot:\n",
    );
    assert!(
        out.contains("BOOT FAIL second boot: no unlock prompt on the console"),
        "{out}"
    );
    assert!(
        out.contains("BOOT FAIL second boot: no failed unit"),
        "{out}"
    );
    assert!(
        out.contains("BOOT FAIL no failed unit after the first boot (tpm)"),
        "{out}"
    );
    // mok without encryption: a second start too, and Secure Boot still on in it.
    let (out, err, starts) = run("no", "mok", good, "login:\n");
    assert_eq!(starts, 3, "{err}\n{out}");
    assert!(
        out.contains("BOOT PASS second boot: Secure Boot still on"),
        "{out}"
    );
    assert!(
        !out.contains("no failed unit after the first boot"),
        "{out}"
    );
    // A plain unencrypted install is started once for the boot, as before.
    let (out, err, starts) = run("no", "0", good, "login:\n");
    assert_eq!(starts, 2, "{err}\n{out}");
    assert!(!out.contains("second boot"), "{out}");
    // WOLOS_PLAN=auto: the plan the app would write goes on the key disk and the live kernel is told
    // where it is; the booted system's account proves it was used. Without the example binary the
    // run refuses before any VM exists.
    let (out, err, starts, calls) = run_p("no", "0", good, "wolos-install: done\nlogin:\n", "auto");
    assert_eq!(starts, 2, "{err}\n{out}\n{calls}");
    assert!(calls.contains("wolos.plan=WOLOSKEY:plan"), "{calls}");
    assert!(calls.contains("probe.json"), "{calls}");
    assert!(
        out.contains("BOOT PASS the plan was used (its account exists)"),
        "{out}"
    );
    assert!(
        out.contains("BOOT PASS plan install said done (wolos.plan=)"),
        "{out}\n{err}"
    );
    let no_account = good.replace("== account\n1\n", "== account\n0\n");
    let (out, _, _, _) = run_p("no", "0", &no_account, "login:\n", "auto");
    assert!(
        out.contains("BOOT FAIL the plan was used (its account exists)"),
        "{out}"
    );
    let o = Command::new("sh")
        .arg(repo("tests/test-install.sh"))
        .arg(&iso)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("WOLOS_VM", d.join("vm"))
        .env("WOLOS_WOLVM", bin.join("wolvm"))
        .env("WOLOS_PLAN", "auto")
        .env_remove("WOLOS_INSTALL_SCENARIO")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("WOLOS_INSTALL_SCENARIO"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let _ = fs::remove_dir_all(&d);
}

/// audit5 I6-7: the dual-boot e2e checks the clock, a BitLocker disk blkid really names so, and
/// a reinstall beside Windows over the old WolOS.
#[test]
fn dual_boot_e2e_checks_clock_bitlocker_and_reinstall() {
    let i = read("tests/test-installer.sh");
    let o = Command::new("bash")
        .arg("-n")
        .arg(repo("tests/test-installer.sh"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    for want in [
        "grep -qx LOCAL '$d/adjtime'",
        "subvol=@wolos-a",
        "bitlocker-windows) echo 1 2 3",
        "bitlocker_header \"$D\" $((p3 * 512))",
        "BitLocker disk refused (no install)",
        "the refusal names BitLocker",
        "reinstall-windows) reinstall",
        r#"install_in_vm "$d" "$S" alongside"#,
        r#"install_in_vm "$d" "$S" replace-wolos"#,
        "Windows partitions byte-identical",
        "the old WolOS startup files replaced",
        "one WolOS entry, none left from the old install",
        // audit6 I6-6: plain Replace over another Linux, and the offline module queue
        "manual|manual-ext4|replace) mk_disk linux",
        "replace) echo 3 4",
        "the replaced partition was rewritten",
        "offline-modules) mk_disk blank",
        "offline_modules()",
        "the pending file emptied once online",
        "the offline start counted no try",
        "--net on",
    ] {
        assert!(i.contains(want), "test-installer.sh lacks {want}");
    }
    assert!(!i.contains("E2E TODO"), "a scenario left as a stub");
    // The reinstall replaces the old WolOS as the installer identifies it, not a renamed partition.
    assert!(
        !i.contains("--part-label"),
        "test-installer.sh renames a partition"
    );
    let sc = read("crates/shell/examples/install_scenario.rs");
    assert!(
        sc.contains("\"replace-wolos\" =>") && sc.contains("p.is_wolos_root()"),
        "install_scenario lacks the replace-wolos scenario"
    );
    assert!(
        sc.contains("\"replace\" =>")
            && sc.contains("\"modules\"")
            && sc.contains("min_by_key(|m| m.size_mb)")
            && sc.contains("println!(\"e2e-module: {id}\")"),
        "install_scenario lacks replace or the modules scenario"
    );
    // The module is one the Modules page offers (f.module_list), not a record id written down: the
    // pre-release run ticked ai-sessions, which the page has no row for, and the install never ran.
    // The harness takes the id from the install log.
    assert!(
        !sc.contains("tick_module(\""),
        "install_scenario ticks a module by a written-down id"
    );
    for want in [
        "grep -a 'e2e-module: ' \"$d/install.log\"",
        "grep -qx '$id' '$d/pending'",
        "pacman -Q wolos-module-$id",
        "no module ticked and installed (install log)",
        // /var is the shared @var subvolume, not part of the slot: the queue is read there (the
        // pre-release run read @wolos-a/var, found nothing and passed the try checks vacuously).
        r#"slot_file "$d" lib/wolos/modules-pending "$d/pending" @var"#,
        r#"slot_file "$d" lib/wolos/modules-attempts "$d/attempts" @var"#,
        r#"mount -o "ro,subvol=$sv""#,
        // A pre-release build's repo is not published: served from this machine, its key trusted,
        // both set in the guest while it is still offline (the drain then runs on connection up).
        "WOLOS_E2E_REPO",
        "python3 -u -m http.server 0",
        "Server = http://10.0.2.2:$port",
        "pacman-key --lsign-key $fpr",
    ] {
        assert!(i.contains(want), "test-installer.sh lacks {want}");
    }
    assert!(
        !i.contains("ai-sessions"),
        "test-installer.sh names a module"
    );
    // The Windows fixture's NTFS boot sector carries its start and geometry, as Windows writes it
    // (mkntfs on a loop partition can not read them and warns that Windows would not boot).
    assert!(
        i.contains(r#"-p "$(cat "/sys/class/block/${LOOP#/dev/}p3/start")" -H 255 -S 63"#),
        "the Windows fixture's mkntfs lacks start and geometry"
    );
    // The fixture's header is one blkid calls BitLocker (the installer's probe reads blkid).
    let have_blkid = Command::new("sh")
        .args(["-c", "command -v blkid"])
        .output()
        .unwrap()
        .status
        .success();
    if !have_blkid {
        eprintln!("no blkid here: the BitLocker fixture check runs on the forge");
        return;
    }
    let d = std::env::temp_dir().join(format!("wol-bitlocker-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let img = d.join("p.img");
    let o = Command::new("bash")
        .arg("-c")
        .arg(r#"eval "$(sed -n '/^bitlocker_header() {/,/^}/p' "$1")" && truncate -s 1M "$2" && printf '\353\122\220NTFS    ' | dd of="$2" bs=1 conv=notrunc status=none && bitlocker_header "$2" 0 && blkid -p -o value -s TYPE "$2""#)
        .arg("bash")
        .arg(repo("tests/test-installer.sh"))
        .arg(&img)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim(),
        "BitLocker",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let _ = fs::remove_dir_all(&d);
}

/// The e2e image tests this tree, not only what its packages carried (F10): remaster lays
/// desktop/overlay down as wolos-system packages it (overlay, then files/ over it, usr/local
/// left out), so a hook such as NetworkManager's 50-wolos-online reaches the image. Run here on
/// a scratch root with the real overlay.
#[test]
fn remaster_copies_the_desktop_overlay_as_the_package_does() {
    let i = read("tests/test-installer.sh");
    let body = i
        .split("remaster() {")
        .nth(1)
        .and_then(|b| b.split("\n}\n").next())
        .expect("remaster() in test-installer.sh");
    let ov = body
        .find(r#"(cd "$SRC/desktop/overlay" && tar --exclude ./usr/local -cf - .)"#)
        .expect("remaster copies desktop/overlay whole");
    let files = body
        .find(r#"sudo cp -r "$SRC"/files/. "$r/""#)
        .expect("remaster copies files/");
    assert!(
        ov < files,
        "files/ goes over the overlay, as in the package"
    );
    assert!(
        !body.contains("desktop/overlay/usr/share/wolos/hypr/"),
        "no hand-picked overlay files left"
    );
    for want in [
        "--no-same-owner --no-overwrite-dir",
        r#"sudo chmod 440 "$r/etc/sudoers" "$r/etc/sudoers.d/wolos""#,
    ] {
        assert!(body.contains(want), "remaster lacks {want}");
    }
    // The overlay copy itself, on a scratch root, sudo a no-op.
    let overlay = siblings::root()
        .join("desktop/overlay")
        .canonicalize()
        .expect("desktop/overlay (the wolos sibling)");
    let src = overlay.parent().and_then(|p| p.parent()).unwrap();
    let d = std::env::temp_dir().join(format!("wol-remaster-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("etc")).unwrap();
    fs::set_permissions(d.join("etc"), fs::Permissions::from_mode(0o750)).unwrap();
    let start = &body[ov..];
    let copy = &start[..start
        .find("|| exit 1")
        .expect("the overlay copy stops on failure")];
    let o = Command::new("bash")
        .arg("-c")
        .arg(format!("set -o pipefail; sudo() {{ \"$@\"; }}; {copy}"))
        .env("SRC", src)
        .env("r", &d)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let hook = d.join("etc/NetworkManager/dispatcher.d/50-wolos-online");
    assert_eq!(
        fs::read(&hook).expect("the dispatcher hook reached the image"),
        fs::read(overlay.join("etc/NetworkManager/dispatcher.d/50-wolos-online")).unwrap()
    );
    assert_eq!(
        fs::metadata(&hook).unwrap().permissions().mode() & 0o111,
        0o111,
        "the hook stays executable"
    );
    assert!(d.join("usr/share/wolos/hypr/installer.conf").is_file());
    assert!(!d.join("usr/local").exists(), "usr/local left out");
    assert_eq!(
        fs::metadata(d.join("etc")).unwrap().permissions().mode() & 0o777,
        0o750,
        "a directory already in the image keeps its mode"
    );
    let _ = fs::remove_dir_all(&d);
}

/// Every booted scenario proves its seal check passed (F10: a check that could not see
/// /usr/lib/modules marked every fresh slot failed, and no scenario looked).
#[test]
fn booted_scenarios_check_the_seal_mark() {
    let i = read("tests/test-installer.sh");
    let after_boot = i
        .split(r#"say "booted (entry blessed)" "blessed '$d'""#)
        .nth(1)
        .expect("scenario() checks the boot");
    let next: String = after_boot.lines().take(4).collect::<Vec<_>>().join("\n");
    assert!(
        next.contains(r#"slot_file "$d" lib/wolos/verity-failed-wolos-a "$d/verity-failed" @var"#)
            && next.contains("[ ! -s '$d/verity-failed' ]"),
        "{next}"
    );
}
