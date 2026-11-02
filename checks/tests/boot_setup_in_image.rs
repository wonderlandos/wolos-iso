// SPDX-License-Identifier: GPL-2.0-or-later
//! What this file checks (wolos-iso): boot-setup in the image: the tools it needs ship and the first stage stays pinned.
//! boot-setup with every tool faked: which chain each tier gets and what it asks of the firmware.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
#[test]
fn the_image_carries_the_tools_and_the_first_stage_stays_pinned() {
    let root = siblings::root_str();
    let list = std::fs::read_to_string(format!("{root}/profile/packages.x86_64")).unwrap();
    for p in ["systemd-ukify", "sbctl", "mokutil", "wolos-shim"] {
        assert!(list.lines().any(|l| l == p), "{p}");
    }
    let repo = std::fs::read_to_string(format!("{root}/packages")).unwrap();
    assert!(repo.lines().any(|l| l.starts_with("wolos-shim ")));
    let notice = std::fs::read_to_string(format!("{root}/NOTICE")).unwrap();
    for line in [
        "Copyright 2012 Red Hat, Inc <mjg@redhat.com>",
        "Redistribution and use in source and binary forms, with or without",
        "OF THE POSSIBILITY OF SUCH DAMAGE.",
        "Corporation.",
    ] {
        assert!(notice.lines().any(|l| l == line), "{line}");
    }
    let b = std::fs::read_to_string(format!("{root}/wolos-shim/PKGBUILD")).unwrap();
    assert!(!b.contains("SKIP"), "every source is pinned");
    assert!(b.contains("351e131d3c4a636704e9cae7d1296baf8a05beca9922cfebbfa48933c9bf1ad3"));
    assert!(
        !b.contains("sbsign"),
        "the first stage is shipped as it was signed"
    );
}
