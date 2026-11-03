// SPDX-License-Identifier: GPL-2.0-or-later
//! `tests/support/siblings.rs` is a verbatim copy of wolos `crates/controller/tests/support/siblings.rs`
//! (one source of truth, no cross-repo crate dependency). This test fails when the copy drifted.
//! Without the wolos sibling checked out next to this repo (or $WOLOS_SRC) there is nothing to compare.
use std::{fs, path::PathBuf};

#[test]
fn support_siblings_is_a_verbatim_copy_of_the_wolos_original() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let wolos =
        std::env::var_os("WOLOS_SRC").map_or_else(|| here.join("../../wolos"), PathBuf::from);
    let original = wolos.join("crates/controller/tests/support/siblings.rs");
    let Ok(want) = fs::read(&original) else {
        eprintln!("skipped: no wolos checkout at {}", wolos.display());
        return;
    };
    let have = fs::read(here.join("tests/support/siblings.rs")).unwrap();
    assert!(
        have == want,
        "tests/support/siblings.rs differs from {}: copy the original over it",
        original.display()
    );
}
