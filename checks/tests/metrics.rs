// SPDX-License-Identifier: GPL-2.0-or-later
//! tests/metrics.sh turns a release run into metrics.json (release/metrics.schema.json; NS2).
//! Why: the site's Proof and Compare pages print these numbers, so the collector must read the
//! right line of the right file, leave what a run did not measure as null (never a guess), and fail
//! when its own output does not match the schema. How: run it on the cut-down run in
//! tests/fixtures/metrics/run (every metric has a known value there, see its README.md), on an
//! empty run (everything null, still valid), and lint it with shellcheck when that is installed.
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// (exit code, stdout, stderr) of `bash tests/metrics.sh RUN`.
fn collect(run: &Path) -> (i32, String, String) {
    let o = Command::new("bash")
        .arg(root().join("tests/metrics.sh"))
        .arg(run)
        .output()
        .expect("bash tests/metrics.sh");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn the_fixture_run_gives_every_frozen_number() {
    let (rc, out, err) = collect(&root().join("tests/fixtures/metrics/run"));
    assert_eq!(rc, 0, "metrics.sh failed: {err}");
    for want in [
        "\"boot_s\": 37.0,",
        "\"boot_systemd_s\": 39.2,",
        "\"install_s\": 151.4,",
        "\"pages\": 3,",
        "\"idle_cpu_ms_s\": 0.34,",
        "\"shell_map_ms\": 237.4,",
        "\"iso_bytes\": 2128447488,",
        "\"total\": 3,",
        "\"passed\": 2,",
        "\"failed_step\": \"4 stop\"",
        "\"date\": \"2026-11-03\"",
        "\"version\": \"0.12.0\"",
        "\"kernel\": \"7.3.0-6-wolos\"",
        "\"webapps\": null",
    ] {
        assert!(out.contains(want), "missing {want} in:\n{out}");
    }
    // Every number has its evidence: the file it came from and the command behind it.
    for key in ["boot_s", "install_s", "pages", "idle_cpu_ms_s", "shell_map_ms", "iso_bytes"] {
        let ev = format!("\"{key}\": {{\n      \"from\": ");
        assert!(out.contains(&ev), "no evidence row for {key}:\n{out}");
    }
}

#[test]
fn a_run_that_measured_nothing_is_all_null_and_still_valid() {
    let dir = std::env::temp_dir().join(format!("wolos-metrics-empty-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("vm")).unwrap();
    let (rc, out, err) = collect(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(rc, 0, "metrics.sh failed: {err}");
    for key in ["boot_s", "install_s", "pages", "idle_cpu_ms_s", "shell_map_ms", "iso_bytes"] {
        assert!(out.contains(&format!("\"{key}\": null,")), "{key} not null:\n{out}");
    }
    assert!(out.contains("\"evidence\": {}"), "evidence without numbers:\n{out}");
}

#[test]
fn output_that_breaks_the_schema_fails_the_run() {
    let dir = std::env::temp_dir().join(format!("wolos-metrics-bad-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("iso")).unwrap();
    std::fs::write(dir.join("iso/x.json"), r#"{"sha256":"not-a-digest","size":1}"#).unwrap();
    let (rc, _, err) = collect(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(rc, 1, "an invalid sha256 must fail: {err}");
    assert!(err.contains("metrics.run.sha256"), "{err}");
}

#[test]
fn a_missing_run_dir_is_a_usage_error() {
    let (rc, _, err) = collect(Path::new("/nonexistent/wolos-run"));
    assert_eq!(rc, 2);
    assert!(err.contains("usage: tests/metrics.sh"), "{err}");
}

#[test]
fn the_schema_freezes_the_five_keys_and_keeps_webapps() {
    let s = std::fs::read_to_string(root().join("release/metrics.schema.json")).unwrap();
    let req = s
        .split("\"required\": [")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .expect("top-level required list");
    for key in ["boot_s", "install_s", "pages", "idle_cpu_ms_s", "shell_map_ms", "webapps"] {
        assert!(req.contains(&format!("\"{key}\"")), "{key} not required by the schema");
    }
}

#[test]
fn metrics_sh_passes_shellcheck() {
    let Ok(o) = Command::new("shellcheck")
        .args(["-S", "style"])
        .arg(root().join("tests/metrics.sh"))
        .output()
    else {
        eprintln!("shellcheck not installed: skipped (CI runs it on tests/*.sh)");
        return;
    };
    assert!(
        o.status.success(),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
}
