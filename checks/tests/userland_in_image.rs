// SPDX-License-Identifier: GPL-2.0-or-later
//! What this file checks (wolos-iso): The image ships the everyday userland (and leaves the Firefox folder alone).
//! Everyday userland from the real-GPU run: sudo for administrators, zsh for every account, user
//! folders, unlocked keyring, printing, removable media and default apps (Atlas the browser).
//! Scripts run on the host with fakes.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, os::unix::fs::PermissionsExt, path::Path, path::PathBuf, process::Command};
fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn packages() -> Vec<String> {
    ["profile/packages.x86_64", "profile/packages.desktop"]
        .iter()
        .flat_map(|f| {
            read(f)
                .lines()
                .filter_map(|l| l.split('#').next())
                .flat_map(|l| l.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn zsh() -> Option<&'static str> {
    let ok = Command::new("zsh").arg("-fc").arg(":").status();
    ok.is_ok_and(|s| s.success()).then_some("zsh")
}

#[test]
fn image_ships_the_everyday_userland() {
    let pkgs = packages();
    for p in [
        "zsh-autosuggestions",
        "zsh-syntax-highlighting",
        "zsh-completions",
        "xdg-user-dirs",
        "gnome-keyring",
        "cups",
        "cups-filters",
        "avahi",
        "nss-mdns",
        "udisks2",
        "gvfs",
        "ntfs-3g",
        "exfatprogs",
        "nautilus",
        "geary",
        "amberol",
        // Photos is Gallery (E2E-SET); Shotwell is in the Store, not on the image.
        "wolos-app-gallery",
        "gnome-calendar",
        "gnote",
        "gnome-calculator",
        "gnome-clocks",
        "papers",
        "celluloid",
        "mpv",
        "file-roller",
        "baobab",
        "gnome-text-editor",
        "eog",
        "atlas-browser",
    ] {
        assert!(pkgs.iter().any(|x| x == p), "packages.txt lacks {p}");
    }
    // Weather is the WolOS desktop widget: gnome-weather needs geoclue and gjs + spidermonkey (~55 MB).
    assert!(!pkgs.iter().any(|x| x == "gnome-weather"));
    // sudo-rs and zsh come with wolos-base (ADR 0017).
    let base = read("wolos-base/PKGBUILD");
    assert!(base.contains("'sudo-rs'") && base.contains("'zsh'"));
}

/// The image changes nothing inside Firefox's own folder; the config waits in its own package,
/// and that package has no mark in its name and is not in the image.
#[test]
fn the_image_leaves_the_firefox_folder_alone() {
    let root = siblings::root();
    assert!(!root.join("desktop/overlay/usr/lib/firefox").exists());
    assert!(root.join("wolos-browser-config/PKGBUILD").is_file());
    for list in ["packages", "profile/packages.x86_64"] {
        let t = std::fs::read_to_string(root.join(list)).unwrap();
        assert!(!t.contains("wolos-browser-config"), "{list}");
    }
}

/// QA-FILES-6: Files reaches phones (MTP), cameras (PTP), Windows shares (SMB) and iPhones (AFC):
/// each gvfs backend is its own Arch package, and gvfs alone has none of them.
#[test]
fn image_ships_the_gvfs_backends() {
    let pkgs = packages();
    for p in ["gvfs", "gvfs-mtp", "gvfs-gphoto2", "gvfs-smb", "gvfs-afc"] {
        assert!(pkgs.iter().any(|x| x == p), "{p} ships on the image");
    }
}

/// Arch builds gvfs with libexecdir=/usr/lib: the daemons are /usr/lib/gvfsd-mtp and the like,
/// /usr/lib/gvfs/ holds only its private libraries. A journey probing /usr/lib/gvfs/gvfsd-* sees
/// "missing" on an image that has them (the 0.13.0 dry run's files-media row).
#[test]
fn journeys_probe_the_gvfs_daemons_where_arch_installs_them() {
    let dir = repo("tests/wolvm/journeys");
    let mut bad = Vec::new();
    for e in fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "yaml") {
            let t = fs::read_to_string(&p).unwrap();
            if t.contains("/usr/lib/gvfs/gvfsd") || t.contains("/usr/lib/gvfs/$b") {
                bad.push(p.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    bad.sort();
    assert!(bad.is_empty(), "journeys probe /usr/lib/gvfs/gvfsd-*: {bad:?}");
}
