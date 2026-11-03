// SPDX-License-Identifier: GPL-2.0-or-later
//! The image ships what the Steam client checks before it starts, a Steam launcher that runs
//! `wol games steam`, and a handler that opens Windows programs on Wine.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::path::PathBuf;
fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    std::fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn steam_and_umu_are_native_packages() {
    let p = read("profile/packages.desktop");
    // .exe outside Steam runs on umu-launcher (Proton, 1 MB); wine (600 MB) installs on demand.
    assert!(!p.lines().any(|l| l.trim() == "wine"), "wine in the image");
    for want in [
        "steam",
        "umu-launcher",
        "lib32-mesa",
        "lib32-vulkan-intel",
        "lib32-vulkan-radeon",
    ] {
        assert!(
            p.lines().any(|l| l.split_whitespace().next() == Some(want)),
            "{want}"
        );
    }
    assert!(read("profile/pacman.conf").contains("\n[multilib]\n"));
}
