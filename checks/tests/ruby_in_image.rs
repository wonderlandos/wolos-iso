// SPDX-License-Identifier: GPL-2.0-or-later
//! What this file checks (wolos-iso): Ruby scripts run on every WolOS like shell scripts (wolos ADR
//! 0041, `wol script`): the interpreter `ruby` is in the image's package lists exactly once, the
//! linter `rubocop` is not (it comes with the Developer module, whose record and meta package list
//! it), and the image still enables no ssh server, so `wol script on` reaches a computer only after
//! its owner turns ssh on.
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf};

fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Every package word in the image lists (comments dropped).
fn packages() -> Vec<String> {
    ["profile/packages.x86_64", "profile/packages.desktop"]
        .iter()
        .flat_map(|f| {
            read(f)
                .lines()
                .filter_map(|l| l.split('#').next())
                .flat_map(|l| l.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn the_image_ships_ruby_once_and_no_linter() {
    let p = packages();
    assert_eq!(
        p.iter().filter(|x| *x == "ruby").count(),
        1,
        "ruby listed once"
    );
    assert!(
        !p.iter().any(|x| x == "rubocop"),
        "rubocop belongs to the dev module"
    );
    let desktop = read("profile/packages.desktop");
    assert!(
        desktop.contains("ADR 0041"),
        "the ruby line says why it is there"
    );
}

#[test]
fn the_developer_module_brings_the_linter() {
    let record = read("files/usr/share/wolos/modules/dev.json");
    assert!(record.contains("\"rubocop\""), "dev.json packages");
    let pkgbuild = read("wolos-module-dev/PKGBUILD");
    let depends: String = pkgbuild
        .split("depends=(")
        .nth(1)
        .and_then(|r| r.split(')').next())
        .unwrap_or("")
        .to_string();
    assert!(
        depends.split_whitespace().any(|d| d == "rubocop"),
        "wolos-module-dev depends: {depends}"
    );
}

#[test]
fn the_image_enables_no_ssh_server() {
    let wants = repo("profile/airootfs/etc/systemd/system/multi-user.target.wants/sshd.service");
    assert!(
        fs::symlink_metadata(&wants).is_err(),
        "sshd must not start by itself: {}",
        wants.display()
    );
}
