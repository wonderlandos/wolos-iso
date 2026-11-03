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
    assert!(i.contains(r#"[ "$2" != tpm ] || tpm=--tpm"#));
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
        // One VM at a time: a leftover VM on the same disk and sockets starves the install.
        "pgrep -x qemu-system-x86",
    ] {
        assert!(t.contains(want), "test-install.sh lacks {want}");
    }
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
        "--latency 64/48000",
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
