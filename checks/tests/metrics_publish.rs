// SPDX-License-Identifier: GPL-2.0-or-later
//! release/metrics-publish.sh RUN SITE puts a release run's numbers into the site's data/metrics.json.
//! Why: the site prints that file as proof, so it may only ever hold what tests/metrics.sh measured
//! and validated; a run that fails validation must leave the site's file untouched. How: run it on
//! the cut-down run in tests/fixtures/metrics/run against a temporary site folder, and lint it.
#[path = "support/siblings.rs"]
mod siblings;

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wolos-publish-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// (exit code, stdout, stderr) of `release/metrics-publish.sh RUN SITE`.
fn publish(run: &Path, site: &Path) -> (i32, String, String) {
    let o = Command::new("bash")
        .arg(root().join("release/metrics-publish.sh"))
        .arg(run)
        .arg(site)
        .output()
        .expect("bash release/metrics-publish.sh");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn the_numbers_of_a_run_land_in_the_sites_data_folder() {
    let site = scratch("ok");
    std::fs::create_dir_all(site.join("data")).unwrap();
    std::fs::write(site.join("data/metrics.json"), "{}\n").unwrap();
    let (rc, out, err) = publish(&root().join("tests/fixtures/metrics/run"), &site);
    let got = std::fs::read_to_string(site.join("data/metrics.json")).unwrap();
    let left: Vec<_> = std::fs::read_dir(site.join("data")).unwrap().collect();
    std::fs::remove_dir_all(&site).ok();
    assert_eq!(rc, 0, "publish failed: {err}");
    for want in [
        "\"boot_s\": 37.0,",
        "\"pages\": 3,",
        "\"idle_cpu_ms_s\": 0.34,",
        "\"shell_map_ms\": 237.4,",
    ] {
        assert!(got.contains(want), "missing {want} in:\n{got}");
    }
    assert!(
        out.contains("measured: boot_s install_s pages idle_cpu_ms_s shell_map_ms"),
        "{out}"
    );
    assert!(out.contains("null: -"), "{out}");
    assert_eq!(left.len(), 1, "a temporary file stayed in data/: {left:?}");
}

#[test]
fn a_run_that_fails_the_schema_leaves_the_sites_file_alone() {
    let run = scratch("bad-run");
    std::fs::create_dir_all(run.join("iso")).unwrap();
    std::fs::write(
        run.join("iso/x.json"),
        r#"{"sha256":"not-a-digest","size":1}"#,
    )
    .unwrap();
    let site = scratch("bad-site");
    std::fs::create_dir_all(site.join("data")).unwrap();
    std::fs::write(site.join("data/metrics.json"), "old\n").unwrap();
    let (rc, _, err) = publish(&run, &site);
    let got = std::fs::read_to_string(site.join("data/metrics.json")).unwrap();
    let left = std::fs::read_dir(site.join("data")).unwrap().count();
    std::fs::remove_dir_all(&run).ok();
    std::fs::remove_dir_all(&site).ok();
    assert_eq!(rc, 1, "an invalid run must fail: {err}");
    assert_eq!(got, "old\n");
    assert_eq!(left, 1, "a temporary file stayed in data/");
    assert!(err.contains("left as it was"), "{err}");
}

#[test]
fn wrong_arguments_are_refused_before_anything_is_written() {
    let site = scratch("args");
    let run = root().join("tests/fixtures/metrics/run");
    let (rc, _, err) = publish(&run, &site); // no data/ folder: not a site checkout
    assert_eq!(rc, 2, "{err}");
    assert!(!site.join("data").exists());
    std::fs::create_dir_all(site.join("data")).unwrap();
    let (rc, _, _) = publish(&site.join("nope"), &site); // no such run
    assert_eq!(rc, 2);
    let o = Command::new("bash")
        .arg(root().join("release/metrics-publish.sh"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(std::fs::read_dir(site.join("data")).unwrap().count(), 0);
    std::fs::remove_dir_all(&site).ok();
}

#[test]
fn metrics_publish_passes_shellcheck() {
    let Ok(o) = Command::new("shellcheck")
        .args(["-S", "style"])
        .arg(root().join("release/metrics-publish.sh"))
        .output()
    else {
        eprintln!("shellcheck not installed: skipped");
        return;
    };
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
}

/// shell_map_ms comes from the shell's own log, which only has stage times when WOL_SHELL_TIMING is
/// set: wol-session (a file of the wolos repo) sets it for every session, and a caller's value wins.
#[test]
fn the_session_turns_the_shell_timing_on() {
    let p = siblings::root().join("desktop/overlay/usr/bin/wol-session");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert!(
        s.lines()
            .any(|l| l.trim() == "export WOL_SHELL_TIMING=${WOL_SHELL_TIMING:-1}"),
        "wol-session does not export WOL_SHELL_TIMING"
    );
    assert!(
        s.contains("wol-shell.log"),
        "the comment must say where the shell's stage times go (hyprland.conf: $XDG_RUNTIME_DIR/wol-shell.log)"
    );
}
