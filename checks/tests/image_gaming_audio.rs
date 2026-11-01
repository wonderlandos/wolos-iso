// SPDX-License-Identifier: GPL-2.0-or-later
//! Wine/Proton and PipeWire defaults ship as plain config values (ported from the CachyOS and
//! Bazzite settings): kernel limits, file-descriptor limits, realtime audio, Bluetooth codecs.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::path::PathBuf;
fn read(p: &str) -> String {
    let f = siblings::root()
        .join(p);
    std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn end_user_image_has_no_toolchain_or_gnu_docs() {
    let all = format!(
        "{}\n{}\n{}",
        read("profile/packages.desktop"),
        read("profile/packages.x86_64"),
        read("profile/packages-base.txt")
    );
    for banned in [
        "gcc",
        "texinfo",
        "man-pages",
        "base-devel",
        "make",
        "autoconf",
    ] {
        assert!(
            !all.lines()
                .any(|l| l.split_whitespace().next() == Some(banned)),
            "{banned} ships on the image"
        );
    }
}

#[test]
fn gamemode_and_rtkit_are_installed() {
    let p = read("profile/packages.desktop");
    for want in ["gamemode", "lib32-gamemode", "rtkit"] {
        assert!(
            p.lines().any(|l| l.split_whitespace().next() == Some(want)),
            "{want}"
        );
    }
}
