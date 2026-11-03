// SPDX-License-Identifier: GPL-2.0-or-later
//! WolOS mirrors (ADR 0030): the keyring package carries the WolOS repo key and the CachyOS key
//! (pinned in wolos-keyring/cachy-trusted) fully trusted; nothing republishes CachyOS packages any
//! more (no mirror-cachy.sh, no cachy-extra, no [wolos-cachy] anywhere in this repo).
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf, process::Command};
fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

const WOLOS_KEY: &str = "C3B6062A2BA6EEABC1DA0F5766C5F2B181374033";
/// The CachyOS package signing key (cachy-trusted pins it; the test cross-checks the constant).
const CACHY_KEY: &str = "882DCFE48E2051D48E2562ABF3B607488DB35A47";

fn fingerprints(p: &str) -> Vec<String> {
    let out = Command::new("gpg")
        .args(["--show-keys", "--with-colons"])
        .arg(repo(p))
        .output()
        .expect("gpg");
    let mut fprs = Vec::new();
    let mut primary = false;
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = l.split(':').collect();
        match f[0] {
            "pub" => primary = true,
            "sub" => primary = false,
            "fpr" if primary => fprs.push(f[9].to_owned()),
            _ => {}
        }
    }
    fprs
}

/// `FPR:4:` lines of a pacman-key trust file (comments skipped).
fn trusted_fprs(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(':').next().unwrap())
        .collect()
}

#[test]
fn keyring_package_carries_repo_and_cachyos_keys_fully_trusted() {
    let cachy = read("wolos-keyring/cachy-trusted");
    let cachy = trusted_fprs(&cachy);
    assert_eq!(cachy, [CACHY_KEY], "one pinned CachyOS key");
    let keys = fingerprints("wolos-keyring/wolos.gpg");
    assert!(keys.iter().any(|k| k == WOLOS_KEY), "{keys:?}");
    assert_eq!(
        fingerprints("wolos-keyring/wolos-repo.pub.asc"),
        [WOLOS_KEY]
    );
    for c in &cachy {
        assert!(keys.iter().any(|k| k == c), "{c} missing from {keys:?}");
    }
    let trusted = read("wolos-keyring/wolos-trusted");
    for k in keys.iter() {
        assert!(
            trusted.contains(&format!("{k}:4:\n")),
            "{k} not fully trusted"
        );
    }
    let pb = read("wolos-keyring/PKGBUILD");
    assert!(pb.contains("$pkgdir/usr/share/pacman/keyrings/wolos.gpg"));
    assert!(pb.contains("$pkgdir/usr/share/pacman/keyrings/wolos-trusted"));
    assert!(pb.contains("install=wolos-keyring.install"));
    assert!(
        pb.contains("cachy-trusted"),
        "the PKGBUILD names the pin file"
    );
    let inst = read("wolos-keyring/wolos-keyring.install");
    assert!(inst.contains("post_install()") && inst.contains("post_upgrade()"));
    assert!(inst.contains("pacman-key --populate wolos"));
}

#[test]
fn keyring_is_built_before_base_and_installed_first() {
    let order = |text: &str, name: &str| {
        text.lines()
            .position(|l| l.split_whitespace().next() == Some(name))
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    let pk = read("packages");
    assert!(order(&pk, "wolos-keyring") < order(&pk, "wolos-base"));
    assert!(pk.contains("\nwolos-keyring "));
    let list = read("profile/packages.x86_64");
    assert!(order(&list, "wolos-keyring") < order(&list, "wolos-base"));
}

/// ADR 0030: CachyOS packages are reached through the CDN's redirect, never copied. The release
/// scripts, the workflow and the image build know nothing of a republished repo.
#[test]
fn nothing_republishes_cachyos_packages() {
    for gone in [
        "release/mirror-cachy.sh",
        "release/cachy-extra",
        "release/cachy-trusted",
    ] {
        assert!(!repo(gone).exists(), "{gone} is back");
    }
    for f in [
        "build.sh",
        "release/release-manifest.sh",
        ".github/workflows/release.yml",
        "profile/pacman.conf",
        "profile/airootfs/etc/pacman.conf",
    ] {
        let t = read(f).to_lowercase();
        assert!(!t.contains("[wolos-cachy]"), "{f}: [wolos-cachy]");
        assert!(!t.contains("wolos_cachy_"), "{f}: WOLOS_CACHY_*");
    }
    assert!(
        !read("release/release-manifest.sh").contains("cachy\""),
        "no cachy field"
    );
}
