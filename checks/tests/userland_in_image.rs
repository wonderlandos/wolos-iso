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
