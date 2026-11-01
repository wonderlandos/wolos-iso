// SPDX-License-Identifier: GPL-2.0-or-later
//! wopkg (ADR 0021) ships in the image: built in the `[wolos]` repo from `wopkg/` and
//! `crates/wopkg`, installed by the image, with libwopkg, its header, completions for zsh, bash and
//! fish, and the official recipes with their index.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf};
fn read(p: &str) -> String {
    let path = siblings::root()
        .join(p);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn wopkg_is_a_repo_package_the_image_installs() {
    let t = read("packages");
    let line = t
        .lines()
        .find(|l| l.starts_with("wopkg "))
        .expect("wopkg in packages");
    let f: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(f[1], "wopkg");
    // Its recipe directory rebuilds it; wopkg/ and crates/wopkg/ of wolos reach it only through the
    // pinned _wolos_commit, a change of that directory (wolos-pkgbuilds `packages`).
    assert!(f[3..].contains(&"wopkg/"), "a change in wopkg/ rebuilds it: {line}");
    let r = read("wopkg/PKGBUILD");
    assert!(
        r.contains("_wolos_commit=") && r.contains("wolos.git#commit=$_wolos_commit"),
        "wopkg/PKGBUILD pins wolos"
    );
    let image = read("profile/packages.x86_64");
    assert!(
        image.lines().any(|l| l.trim() == "wopkg"),
        "the image installs wopkg"
    );
}
