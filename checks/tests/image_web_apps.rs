// SPDX-License-Identifier: GPL-2.0-or-later
//! System web apps on the image (ADR 0048, QA-WEB-4). Doctor and the kit are text (wolos-webapps,
//! under 1 MiB), but they run only on the engine library of webengine-wolos, which is large (the
//! engine is about 900 crates and a JavaScript engine; never measured on the forge yet). The
//! engine enters `profile/packages.*` only with its measured size recorded in `profile/web-engine-size`
//! (a number to read, not a cap: ISO-GATE, lead 2026-11-04, the image has no size limit and build.sh
//! prints the size without failing on it), and the two packages enter together: a launcher whose
//! engine is absent opens nothing.
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
        // The size is recorded, not capped: the image has no size limit (ISO-GATE).
        assert!(
            mib > 0.0,
            "profile/web-engine-size: installed-mib must be positive"
        );
    }
}

/// ISO-GATE: the image has no size limit. build.sh prints the size and the GitHub asset note
/// (release/iso-size.sh); a 2.1 GiB image passes with the note, it is not a failure.
#[test]
fn the_build_never_fails_on_the_iso_size() {
    let b = read("build.sh");
    assert!(
        !b.contains("$((2047 * 1024 * 1024))") && !b.contains("release asset limit"),
        "build.sh still has the ISO size gate"
    );
    assert!(
        b.contains("release/iso-size.sh"),
        "build.sh does not print the ISO size"
    );
    // A fake 2.1 GiB image (a sparse file): exit 0, size and the CDN note printed.
    let d = std::env::temp_dir().join(format!("wolos-iso-size-{}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    let iso = d.join("wolos-0.12.0-x86_64.iso");
    fs::File::create(&iso)
        .unwrap()
        .set_len(2200 * 1024 * 1024)
        .unwrap();
    let out = std::process::Command::new("sh")
        .arg(siblings::root().join("release/iso-size.sh"))
        .arg(&iso)
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "iso-size.sh failed on 2.1 GiB: {text}"
    );
    assert!(text.contains("2306867200 bytes"), "{text}");
    assert!(text.contains("2.1 GiB") && text.contains("CDN"), "{text}");
    // A small one: the size, no GitHub note.
    fs::File::create(&iso)
        .unwrap()
        .set_len(1500 * 1024 * 1024)
        .unwrap();
    let out = std::process::Command::new("sh")
        .arg(siblings::root().join("release/iso-size.sh"))
        .arg(&iso)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success() && text.contains("1.5 GiB") && !text.contains("CDN"),
        "{text}"
    );
    let _ = fs::remove_dir_all(&d);
}

/// QA-WEB-2: the theme journey called `wol theme accent`, which is no verb (the accent is `wol accent
/// set <accent>`, appearance `wol theme dark|light`); the fallback after `||` hid it. Every `wol`
/// command the journey runs is one the controller's command tree has.
#[test]
fn the_web_app_theme_journey_uses_verbs_that_exist() {
    let j = read("tests/wolvm/journeys/qa100-webapp-doctor-theme.yaml");
    let cli = read("crates/controller/src/cli.rs");
    assert!(cli.contains(r#"&["accent", "set"]"#) && cli.contains(r#"&["theme", "dark"]"#));
    assert!(
        !cli.contains(r#"&["theme", "accent"]"#),
        "if it exists now, the journey may use it"
    );
    assert!(!j.contains("wol theme accent"), "no such verb");
    assert!(j.contains("wol accent set green") && j.contains("wol accent set violet"));
}
