// SPDX-License-Identifier: GPL-2.0-or-later
//! System web apps on the image (ADR 0048, QA-WEB-4). Doctor and the kit are text (wolos-webapps,
//! under 1 MiB), but they run only on the engine library of webengine-wolos, which is large (the
//! engine is about 900 crates and a JavaScript engine; never measured on the forge yet). The ISO has
//! 21 MiB of room under the 2047 MiB release asset limit (prerelease run 2026-11-03), so the engine
//! enters `profile/packages.*` only with its measured size recorded in `profile/web-engine-size`,
//! and the two packages enter together: a launcher whose engine is absent opens nothing.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::fs;
fn read(p: &str) -> String {
    let f = siblings::root().join(p);
    fs::read_to_string(&f).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Package names listed (one per line, comments cut) in the profile's two lists.
fn listed() -> Vec<String> {
    ["profile/packages.x86_64", "profile/packages.desktop"]
        .iter()
        .flat_map(|f| {
            read(f)
                .lines()
                .map(|l| l.split('#').next().unwrap_or("").trim().to_owned())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn the_web_apps_and_their_engine_enter_the_image_together_with_a_measured_size() {
    let l = listed();
    let has = |n: &str| l.iter().any(|p| p == n);
    assert_eq!(
        has("wolos-webapps"),
        has("webengine-wolos"),
        "wolos-webapps needs webengine-wolos on the image, and the engine alone shows nothing"
    );
    if has("webengine-wolos") {
        let size = siblings::root().join("profile/web-engine-size");
        let text = fs::read_to_string(&size)
            .unwrap_or_else(|_| panic!("webengine-wolos is on the image: record its installed size in profile/web-engine-size"));
        let mib: f64 = text
            .lines()
            .find_map(|l| l.strip_prefix("installed-mib:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or_else(|| panic!("profile/web-engine-size: a line `installed-mib: N`"));
        // The compressed image takes roughly half of it; the room is 21 MiB (build.sh stops at 2047).
        assert!(
            mib > 0.0 && mib / 2.0 <= 21.0,
            "the engine ({mib} MiB installed) does not fit the ISO's room"
        );
    }
}

#[test]
fn the_build_still_stops_over_the_release_asset_limit() {
    assert!(
        read("build.sh").contains("$((2047 * 1024 * 1024))"),
        "the ISO size limit is what keeps a big package out"
    );
}
