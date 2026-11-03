// SPDX-License-Identifier: GPL-2.0-or-later
//! What this file checks (wolos-iso): sshd on the live medium starts only when the boot carries a
//! root key as the systemd credential `ssh.authorized_keys.root` (wolvm --ssh), never by default,
//! and none of it reaches the installed system.
#![allow(dead_code)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf};

fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

const UNITS: &str = "profile/airootfs/etc/systemd/system";

/// Values of KEY in SECTION of a unit file.
fn values(unit: &str, section: &str, key: &str) -> Vec<String> {
    let mut sect = "";
    let mut v = vec![];
    for l in unit.lines().map(str::trim) {
        if l.starts_with('[') {
            sect = l;
        } else if sect == format!("[{section}]")
            && let Some((k, val)) = l.split_once('=')
            && k == key
        {
            v.push(val.to_owned());
        }
    }
    v
}

#[test]
fn live_sshd_runs_only_with_the_root_key_credential() {
    let d = read(&format!("{UNITS}/sshd.service.d/wolos-live.conf"));
    assert_eq!(
        values(&d, "Unit", "ConditionCredential"),
        ["ssh.authorized_keys.root"],
        "{d}"
    );
    // The key lands in /root/.ssh before sshd (systemd's provision.conf), the firewall rule after
    // nftables loaded its table.
    let after = values(&d, "Unit", "After").join(" ");
    assert!(
        after.contains("systemd-tmpfiles-setup.service") && after.contains("nftables.service"),
        "{after}"
    );
    assert_eq!(
        values(&d, "Service", "ExecStartPre"),
        ["-/usr/bin/nft add rule inet wolos input tcp dport 22 accept"]
    );
    let t = read(&format!("{UNITS}/multi-user.target.d/wolos-live-ssh.conf"));
    assert_eq!(values(&t, "Unit", "Wants"), ["sshd.service"], "{t}");
    // Never default-on: no enablement symlink, no preset line.
    assert!(
        !repo(&format!("{UNITS}/multi-user.target.wants/sshd.service")).exists(),
        "sshd enabled unconditionally on the live medium"
    );
    let preset = read("files/usr/lib/systemd/system-preset/50-wolos.preset");
    assert!(
        !preset.lines().any(|l| l.starts_with("enable sshd")),
        "sshd enabled by the preset"
    );
    // Live only: install-run removes both files from the installed system.
    let run = read("files/usr/lib/wolos/install-run");
    for f in [
        "\"$T/etc/systemd/system/sshd.service.d/wolos-live.conf\"",
        "\"$T/etc/systemd/system/multi-user.target.d/wolos-live-ssh.conf\"",
    ] {
        assert!(run.contains(f), "install-run keeps {f}");
    }
    // sshd is in the image on purpose, not by some dependency's grace.
    assert!(
        read("profile/packages.x86_64")
            .lines()
            .any(|l| l.trim() == "openssh"),
        "openssh not listed in profile/packages.x86_64"
    );
    // The harness passes exactly this credential name.
    if let Ok(w) = fs::read_to_string(repo("tests/wolvm/wolvm")) {
        assert!(
            w.contains("ssh.authorized_keys.root="),
            "wolvm --ssh credential renamed"
        );
    }
}
