// SPDX-License-Identifier: GPL-2.0-or-later
//! Scripts keep their executable bit (audit5 X6-3; the same guard as wolos
//! crates/controller/tests/exec_bits.rs). This repo is checked out with core.filemode=false, so git
//! never sees a new script's +x and a plain `git add` commits it as 100644; a Linux checkout then
//! cannot run it ("permission denied", or a check that quietly turns a feature off: a 0644
//! release/sb-key.sh turns the Secure Boot layout off without a word). checks/exec-bits.txt lists
//! every file of this repo whose first line is a `#!` shebang. These tests keep that list complete,
//! sorted and unique, and check that the git index holds each listed file as 100755. Without a git
//! checkout (a copied tree) the index test skips; files git does not track yet are only reported:
//! `git add --chmod=+x` them.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The list, repo-relative.
const LIST: &str = "checks/exec-bits.txt";
/// Build output the walk skips without git (.gitignore'd; `*` is any one path segment).
const NOT_SOURCE: &[&str] = &["out", "work"];
/// Scripts other code runs by path: build.sh and wolos-build run release/sb-key.sh and read a
/// failure as "no key" (the signed layout goes off), the install tests start tests/wolvm/wolvm.
const RUN_BY_PATH: &[&str] = &["build.sh", "release/sb-key.sh", "tests/wolvm/wolvm"];
/// Tracked as 100644 before the list existed; the commit that adds this test sets their bit
/// (`git update-index --chmod=+x`). Only shrinks: an entry that is 100755 is reported for removal.
const PENDING_CHMOD: &[&str] = &[
    "tests/test/greeter-sway.sh",
    "tests/test/limits/guest-run.sh",
    "tests/test/limits/load.sh",
    "tests/test/limits/matrix.sh",
    "tests/test/limits/mkguest.sh",
    "tests/test/limits/pack.sh",
    "tests/test/limits/report.py",
    "tests/test/shell-shots.sh",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The paths the list names, in file order.
fn listed() -> Vec<String> {
    let t = std::fs::read_to_string(root().join(LIST)).expect(LIST);
    t.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(args)
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Every file git tracks or does not ignore, with its index mode (None: untracked). None without a
/// git checkout (the forge's copies, a source tarball): the callers fall back or skip.
fn git_files() -> Option<BTreeMap<String, Option<String>>> {
    if !root().join(".git").exists() {
        return None;
    }
    let mut m = BTreeMap::new();
    for e in git(&["ls-files", "-s", "-z"])?
        .split('\0')
        .filter(|e| !e.is_empty())
    {
        let (meta, path) = e.split_once('\t')?;
        let mode = meta.split(' ').next()?;
        if mode != "160000" {
            m.insert(path.to_string(), Some(mode.to_string()));
        }
    }
    for p in git(&["ls-files", "-o", "--exclude-standard", "-z"])?
        .split('\0')
        .filter(|p| !p.is_empty())
    {
        m.entry(p.to_string()).or_insert(None);
    }
    Some(m)
}

/// `rel` matches `pattern` segment by segment (`*` is any one segment).
fn matches(pattern: &str, rel: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').collect();
    let r: Vec<&str> = rel.split('/').collect();
    p.len() == r.len() && p.iter().zip(&r).all(|(a, b)| *a == "*" || a == b)
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(rel) = p.strip_prefix(root()) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let n = e.file_name().to_string_lossy().into_owned();
        if matches!(n.as_str(), "target" | ".git" | "node_modules" | ".work")
            || NOT_SOURCE.iter().any(|s| matches(s, &rel))
        {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(rel);
        }
    }
}

/// `#!/...` or `#! ...` (a Rust `#![attr]` is not a shebang).
fn shebang(rel: &str) -> bool {
    use std::io::Read;
    let mut b = [0u8; 3];
    std::fs::File::open(root().join(rel))
        .and_then(|mut f| f.read_exact(&mut b))
        .is_ok()
        && (&b == b"#!/" || &b == b"#! ")
}

/// Every shebang file of the repo.
fn scripts() -> BTreeSet<String> {
    let files: Vec<String> = match git_files() {
        Some(m) => m.into_keys().collect(),
        None => {
            let mut v = Vec::new();
            walk(&root(), &mut v);
            v
        }
    };
    files
        .into_iter()
        .filter(|f| root().join(f).is_file() && shebang(f))
        .collect()
}

fn lines(v: &[&String]) -> String {
    v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_list_names_every_script_and_nothing_else() {
    let listed: BTreeSet<String> = listed().into_iter().collect();
    let found = scripts();
    assert!(found.len() > 20, "only {} scripts found", found.len());
    for f in RUN_BY_PATH {
        assert!(listed.contains(*f), "{f}: run by path, must be listed");
    }
    let missing: Vec<&String> = found.difference(&listed).collect();
    let stale: Vec<&String> = listed.difference(&found).collect();
    assert!(
        missing.is_empty(),
        "shebang files missing from {LIST} (add each, commit it with `git add --chmod=+x`):\n{}",
        lines(&missing)
    );
    assert!(
        stale.is_empty(),
        "{LIST} lists files that are gone or have no shebang:\n{}",
        lines(&stale)
    );
}

#[test]
fn the_list_is_sorted_and_unique() {
    let l = listed();
    let mut s = l.clone();
    s.sort();
    s.dedup();
    assert_eq!(l, s, "{LIST}: keep it sorted, without duplicates");
    let mut p = PENDING_CHMOD.to_vec();
    p.sort_unstable();
    p.dedup();
    assert_eq!(
        p, PENDING_CHMOD,
        "PENDING_CHMOD: sorted, without duplicates"
    );
    for f in PENDING_CHMOD.iter().chain(RUN_BY_PATH) {
        assert!(
            !PENDING_CHMOD.contains(f) || !RUN_BY_PATH.contains(f),
            "{f}: run by path, it may not wait for its bit"
        );
        assert!(
            l.iter().any(|x| x == f),
            "{f} is named here, but the list does not"
        );
    }
}

#[test]
fn listed_scripts_are_committed_executable() {
    let Some(index) = git_files() else {
        eprintln!("no git checkout here; index modes not checked");
        return;
    };
    let mut bad = Vec::new();
    let mut untracked = Vec::new();
    for f in listed() {
        match index.get(&f) {
            Some(Some(mode)) if mode == "100755" => {
                if PENDING_CHMOD.contains(&f.as_str()) {
                    eprintln!("exec bits: {f} is 100755 now; remove it from PENDING_CHMOD");
                }
            }
            Some(Some(mode)) if !PENDING_CHMOD.contains(&f.as_str()) => {
                bad.push(format!("{f} ({mode}): git update-index --chmod=+x {f}"));
            }
            Some(None) => untracked.push(f),
            _ => {}
        }
    }
    if !untracked.is_empty() {
        eprintln!(
            "exec bits: not tracked yet, add with `git add --chmod=+x`:\n{}",
            untracked.join("\n")
        );
    }
    assert!(
        bad.is_empty(),
        "scripts the index holds without the executable bit (from the repo root):\n{}",
        bad.join("\n")
    );
}
