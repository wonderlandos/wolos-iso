// SPDX-License-Identifier: GPL-2.0-or-later
//! VM scripts on a Linux host (ADR 0016): QEMU/KVM with OVMF and virtio-gpu (GL), so Hyprland runs
//! in the VM. build.sh fails fast short of disk; test-install.sh installs unattended from the ISO
//! (wolos.autoinstall) next to a bystander disk that must come out byte-identical.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
fn repo(p: &str) -> PathBuf {
    siblings::root()
        .join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn scripts_parse_and_freebsd_vm_tooling_is_gone() {
    for s in [
        "tests/build.sh",
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
    let out = Command::new("sh")
        .arg(repo("tests/build.sh"))
        .env("PATH", format!("{}:/usr/bin:/bin", d.join("bin").display()))
        .env("WOLOS_VM", &d)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("host has 5000 MB free"), "{err}");
    assert!(!err.contains("mkarchiso-called"), "{err}");
}

#[test]
fn vms_use_ovmf_kvm_and_virtio_gpu() {
    for s in [
        "tests/run.sh",
        "tests/run-installed.sh",
        "tests/test-install.sh",
    ] {
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
        // Interactive VMs draw with GL; the unattended test takes QMP screenshots, which cannot
        // read a GL scanout, so it uses the 2D device.
        let gpu = if s.ends_with("test-install.sh") {
            "-device virtio-vga "
        } else {
            "virtio-vga-gl"
        };
        assert!(t.contains(gpu), "{s} lacks {gpu}");
    }
}

#[test]
fn install_test_is_unattended_and_proves_the_bystander_disk_untouched() {
    let t = read("tests/test-install.sh");
    for want in [
        "wolos.autoinstall=/dev/vda",
        "console=ttyS0",
        "archisobasedir=wolos",
        "-kernel",
        "-initrd",
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
    assert!(!t.contains("reset --yes"), "the guest test never resets for real");
}
