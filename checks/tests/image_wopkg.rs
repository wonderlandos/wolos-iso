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
    for p in ["wopkg/", "wopkg/", "crates/wopkg/"] {
        assert!(f[3..].contains(&p), "a change in {p} rebuilds it: {line}");
    }
    let image = read("profile/packages.x86_64");
    assert!(
        image.lines().any(|l| l.trim() == "wopkg"),
        "the image installs wopkg"
    );
}
