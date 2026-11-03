// SPDX-License-Identifier: GPL-2.0-or-later
//! What this file checks (wolos-iso): the live medium's Secure Boot path (ADR 0025).
//! `release/sb-layout.sh sign` signs systemd-boot and the UKI with the live-image key and puts its
//! DER certificate where the blue key screen's "Enroll key from disk" finds it at once
//! (`/ENROL_THIS_KEY_FOR_WOLOS.cer`, also `EFI/wolos/<name>.cer`) next to `EFI/wolos/README.txt`
//! in the installer's words; `sb-layout.sh check` (what `wolos-build release` runs) passes only
//! then. Fixture: a small partitioned disk whose second partition is a FAT EFI partition, as in
//! the hybrid ISO, with the host's systemd-boot as every EFI binary and keys made here. The live
//! key starts the medium only: nothing of the installed system names it.
#![allow(dead_code)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-sb-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run(cmd: &str, args: &[&str]) -> Output {
    Command::new(cmd)
        .args(args)
        .env("MTOOLS_SKIP_CHECK", "1")
        .output()
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn ok(cmd: &str, args: &[&str]) -> Output {
    let o = run(cmd, args);
    assert!(
        o.status.success(),
        "{cmd} {args:?}: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    o
}

fn have(t: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {t}")])
        .output()
        .unwrap()
        .status
        .success()
}

const SD_BOOT: &str = "/usr/lib/systemd/boot/efi/systemd-bootx64.efi";
const ENROL: &str = "ENROL_THIS_KEY_FOR_WOLOS.cer";

/// The tools the fixture needs; false (and a note) where the host lacks one: the forge has all.
fn tools() -> bool {
    for t in [
        "sfdisk", "mformat", "mcopy", "sbsign", "sbverify", "sbattach", "openssl", "python3",
    ] {
        if !have(t) {
            eprintln!("live_secureboot: no {t} here, fixture test skipped (runs on the forge)");
            return false;
        }
    }
    if !Path::new(SD_BOOT).is_file() {
        eprintln!("live_secureboot: no {SD_BOOT}, fixture test skipped");
        return false;
    }
    true
}

/// A throwaway RSA key + self-signed certificate (KEY, CERT) named CN in DIR.
fn keypair(d: &Path, cn: &str) -> (String, String) {
    let k = d.join(format!("{cn}.key")).display().to_string();
    let c = d.join(format!("{cn}.crt")).display().to_string();
    ok(
        "openssl",
        &[
            "req",
            "-new",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-sha256",
            "-days",
            "1",
            "-subj",
            &format!("/CN={cn}/"),
            "-keyout",
            &k,
            "-out",
            &c,
        ],
    );
    (k, c)
}

/// The EFI partition's first sector in the fixture disk (partition 2, as in the hybrid ISO).
const ESP_START: u64 = 4096;

/// A disk like the hybrid ISO: partition 2 is the EFI partition (FAT), laid out as
/// `sb-layout.sh layout` leaves it, with SHIM as EFI/BOOT/BOOTX64.EFI.
fn medium(d: &Path, shim: &Path) -> String {
    let f = d.join("esp.img").display().to_string();
    let m = d.join("medium.img").display().to_string();
    ok("truncate", &["-s", "16M", &f]);
    ok("mformat", &["-i", &f, "-v", "WOLTEST", "::"]);
    ok(
        "mmd",
        &[
            "-i",
            &f,
            "::/EFI",
            "::/EFI/BOOT",
            "::/EFI/Linux",
            "::/loader",
        ],
    );
    ok(
        "mcopy",
        &["-i", &f, shim.to_str().unwrap(), "::/EFI/BOOT/BOOTX64.EFI"],
    );
    ok("mcopy", &["-i", &f, SD_BOOT, "::/EFI/BOOT/grubx64.efi"]);
    ok("mcopy", &["-i", &f, SD_BOOT, "::/EFI/Linux/wolos.efi"]);
    ok("truncate", &["-s", "20M", &m]);
    let mut p = Command::new("sfdisk")
        .args(["-q", &m])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    p.stdin
        .take()
        .unwrap()
        .write_all(
            format!("label: dos\nstart=2048, size=1024, type=83\nstart={ESP_START}, size=32768, type=ef\n")
                .as_bytes(),
        )
        .unwrap();
    assert!(p.wait().unwrap().success(), "sfdisk");
    ok(
        "dd",
        &[
            &format!("if={f}"),
            &format!("of={m}"),
            "bs=512",
            &format!("seek={ESP_START}"),
            "conv=notrunc",
            "status=none",
        ],
    );
    m
}

fn esp(m: &str) -> String {
    format!("{m}@@{}", ESP_START * 512)
}

fn sb(args: &[&str]) -> Output {
    Command::new(repo("release/sb-layout.sh"))
        .args(args)
        .output()
        .unwrap()
}

/// `sb-layout.sh` with WOLOS_SB_PIN_CERT=PIN: the dev/test override of the pinned live key.
fn sb_pin(args: &[&str], pin: &str) -> Output {
    Command::new(repo("release/sb-layout.sh"))
        .args(args)
        .env("WOLOS_SB_PIN_CERT", pin)
        .output()
        .unwrap()
}

fn sb_key(args: &[&str]) -> Output {
    Command::new(repo("release/sb-key.sh"))
        .args(args)
        .output()
        .unwrap()
}

fn get(m: &str, path: &str, to: &Path) {
    let _ = fs::remove_file(to);
    ok(
        "mcopy",
        &[
            "-n",
            "-i",
            &esp(m),
            &format!("::{path}"),
            to.to_str().unwrap(),
        ],
    );
}

fn signatures(f: &Path) -> usize {
    let o = run("sbverify", &["--list", f.to_str().unwrap()]);
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| l.starts_with("signature "))
        .count()
}

/// SRC with its `.sbat` section renamed (the section table is in the headers, before any data).
fn without_sbat(src: &Path, to: &Path) {
    let mut b = fs::read(src).unwrap();
    let i = b
        .windows(8)
        .position(|w| w == b".sbat\0\0\0")
        .expect("systemd-boot has an .sbat section");
    b[i + 1] = b'x';
    fs::write(to, b).unwrap();
}

#[test]
fn sign_puts_signatures_and_the_der_certificate_on_the_medium_and_check_passes() {
    if !tools() {
        return;
    }
    let d = scratch("sign");
    // The stand-in for the signed shim: systemd-boot (it has .sbat) signed by another key.
    let (vk, vc) = keypair(&d, "vendor");
    let shim = d.join("shim.efi");
    ok(
        "sbsign",
        &[
            "--key",
            &vk,
            "--cert",
            &vc,
            "--output",
            shim.to_str().unwrap(),
            SD_BOOT,
        ],
    );
    let m = medium(&d, &shim);
    // Laid out but unsigned: refused, every missing piece named.
    let o = sb(&["check", &m]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "unsigned layout passed check");
    assert!(err.contains(&format!("no /{ENROL}")), "{err}");
    assert!(err.contains("EFI/BOOT/grubx64.efi is not signed"), "{err}");
    assert!(err.contains("EFI/Linux/wolos.efi is not signed"), "{err}");

    let (k, c) = keypair(&d, "WolOS Live Test");
    let o = sb(&["sign", &m, &k, &c]);
    assert!(o.status.success(), "sign: {o:?}");
    let x = d.join("x.efi");
    for f in ["/EFI/BOOT/grubx64.efi", "/EFI/Linux/wolos.efi"] {
        get(&m, f, &x);
        ok("sbverify", &["--cert", &c, x.to_str().unwrap()]);
        assert_eq!(signatures(&x), 1, "{f}");
    }
    // The DER certificate, at the root and in EFI/wolos, is the signing certificate.
    let der = d.join("want.cer");
    ok(
        "openssl",
        &[
            "x509",
            "-in",
            &c,
            "-outform",
            "DER",
            "-out",
            der.to_str().unwrap(),
        ],
    );
    let name = String::from_utf8(sb_key(&["name"]).stdout).unwrap();
    let got = d.join("got.cer");
    for f in [
        format!("/{ENROL}"),
        format!("/EFI/wolos/{}.cer", name.trim()),
    ] {
        get(&m, &f, &got);
        assert_eq!(fs::read(&got).unwrap(), fs::read(&der).unwrap(), "{f}");
    }
    // The README: the installer's words for the screen, the file to pick, what to press.
    let r = d.join("README.txt");
    get(&m, "/EFI/wolos/README.txt", &r);
    let readme = fs::read_to_string(&r).unwrap();
    assert!(readme.contains("\r\n"), "DOS line ends");
    let flat = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    let copy = read("crates/shell/src/copy_installer.rs");
    let looks = copy
        .lines()
        .find(|l| l.starts_with("pub const KEY_SCREEN_LOOKS"))
        .unwrap()
        .split('"')
        .nth(1)
        .unwrap();
    assert!(
        flat.contains(looks),
        "README lacks KEY_SCREEN_LOOKS: {flat}"
    );
    for want in [
        ENROL,
        "one blue screen",
        "once",
        "Enroll key from disk",
        "Secure Boot off: nothing changes",
        "Reboot",
        "your USB stick",
    ] {
        assert!(flat.contains(want), "README lacks {want:?}: {flat}");
    }
    // I6-6: on a PC the internal (Windows) EFI partition is usually listed first.
    assert!(
        !flat.contains("first volume"),
        "README says first volume: {flat}"
    );
    let o = sb_pin(&["check", &m], &c);
    assert!(o.status.success(), "signed medium fails check: {o:?}");
    // Without the override the pin is release/wolos-live-2026.cer: a throwaway key is refused
    // (I6-1: what `wolos-build release` runs; it never sets WOLOS_SB_PIN_CERT).
    let o = Command::new(repo("release/sb-layout.sh"))
        .args(["check", &m])
        .env_remove("WOLOS_SB_PIN_CERT")
        .output()
        .unwrap();
    assert!(
        !o.status.success(),
        "a medium signed by another key passed check"
    );
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains(&format!("/{ENROL} is not the WolOS live key")),
        "{err}"
    );
    assert!(
        err.contains("EFI/wolos/wolos-live-2026.cer is not the WolOS live key"),
        "{err}"
    );

    // Signed again (release.yml after the forge): one signature each, still passes.
    let o = sb(&["sign", &m, &k, &c]);
    assert!(o.status.success(), "re-sign: {o:?}");
    get(&m, "/EFI/BOOT/grubx64.efi", &x);
    assert_eq!(signatures(&x), 1, "re-signed grubx64.efi");
    assert!(sb_pin(&["check", &m], &c).status.success());

    // Another key's certificate at the root: the second stage no longer matches it.
    let (_, oc) = keypair(&d, "other");
    let ocer = d.join("other.cer");
    ok(
        "openssl",
        &[
            "x509",
            "-in",
            &oc,
            "-outform",
            "DER",
            "-out",
            ocer.to_str().unwrap(),
        ],
    );
    ok(
        "mcopy",
        &[
            "-o",
            "-i",
            &esp(&m),
            ocer.to_str().unwrap(),
            &format!("::/{ENROL}"),
        ],
    );
    let o = sb_pin(&["check", &m], &c);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains(&format!(
            "EFI/BOOT/grubx64.efi is not signed by the certificate in /{ENROL}"
        )),
        "{err}"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn check_refuses_an_unsigned_loader_or_one_without_sbat() {
    if !tools() {
        return;
    }
    let d = scratch("check");
    let (k, c) = keypair(&d, "WolOS Live Test");
    // Unsigned systemd-boot as the firmware's loader (the v0.11.5 ISO).
    let m = medium(&d, Path::new(SD_BOOT));
    assert!(sb(&["sign", &m, &k, &c]).status.success());
    let o = sb_pin(&["check", &m], &c);
    assert!(!o.status.success(), "unsigned BOOTX64.EFI passed");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("EFI/BOOT/BOOTX64.EFI carries no signature"),
        "{err}"
    );
    // Signed, but no .sbat section: not the shim.
    let nosbat = d.join("nosbat.efi");
    without_sbat(Path::new(SD_BOOT), &nosbat);
    let shim = d.join("shim.efi");
    ok(
        "sbsign",
        &[
            "--key",
            &k,
            "--cert",
            &c,
            "--output",
            shim.to_str().unwrap(),
            nosbat.to_str().unwrap(),
        ],
    );
    let d2 = d.join("m2");
    fs::create_dir_all(&d2).unwrap();
    let m = medium(&d2, &shim);
    assert!(sb(&["sign", &m, &k, &c]).status.success());
    let o = sb_pin(&["check", &m], &c);
    assert!(!o.status.success(), "BOOTX64.EFI without .sbat passed");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("has no .sbat section"), "{err}");
    assert!(!err.contains("carries no signature"), "{err}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn the_repo_carries_the_live_certificate_and_never_a_private_key() {
    let name = String::from_utf8(sb_key(&["name"]).stdout).unwrap();
    let name = name.trim();
    assert_eq!(name, "wolos-live-2026");
    // The forge's key, where wolos-build docs/signing-kms.md says it is.
    assert!(read("release/sb-key.sh").contains("KEY=/etc/wolos-forge/sb/live-2026.key"));
    let crt = repo(&format!("release/{name}.crt"));
    let pem = read(&format!("release/{name}.crt"));
    assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n") && !pem.contains("PRIVATE"));
    let der = fs::read(repo(&format!("release/{name}.cer"))).unwrap();
    assert_eq!(der[0], 0x30, "DER: a SEQUENCE");
    // `cert` names the PEM next to the script.
    let c = Command::new(repo("release/sb-key.sh"))
        .arg("cert")
        .env_remove("WOLOS_SB_CERT")
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&c.stdout)
            .trim()
            .ends_with(&format!("release/{name}.crt"))
    );
    for e in fs::read_dir(repo("release")).unwrap().flatten() {
        let b = fs::read(e.path()).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&b).contains("PRIVATE KEY-----"),
            "{}: a private key in the repo",
            e.path().display()
        );
    }
    // The DER and the PEM are the same certificate: CN and ten years.
    if have("openssl") {
        let o = ok(
            "openssl",
            &["x509", "-in", crt.to_str().unwrap(), "-outform", "DER"],
        );
        assert_eq!(o.stdout, der);
        let s = ok(
            "openssl",
            &[
                "x509",
                "-in",
                crt.to_str().unwrap(),
                "-noout",
                "-subject",
                "-enddate",
            ],
        );
        let s = String::from_utf8_lossy(&s.stdout);
        assert!(s.contains("WolOS Live Image 2026"), "{s}");
        assert!(s.contains("2036"), "10 years: {s}");
    }
}

/// Every file under DIR (recursively), as (path, bytes).
fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if let Ok(b) = fs::read(&p) {
            out.push((p, b));
        }
    }
}

#[test]
fn the_live_key_signs_the_live_medium_only() {
    // ADR 0025: the installed system's `mok` tier signs with the machine's own key (boot-keys);
    // the live-image key, its certificate and its forge path appear nowhere in the installed
    // system's files, and its private key path nowhere in the live image either.
    let mut files = vec![];
    walk(&repo("files"), &mut files);
    assert!(files.len() > 20, "wolos files/ not found next to wolos-iso");
    for (p, b) in &files {
        let s = String::from_utf8_lossy(b);
        for bad in [
            "wolos-live-2026",
            "WolOS Live Image",
            "/etc/wolos-forge",
            "sb-key.sh",
            ENROL,
        ] {
            assert!(
                !s.contains(bad),
                "{}: names the live-image key ({bad})",
                p.display()
            );
        }
    }
    let mut live = vec![];
    walk(&repo("profile"), &mut live);
    for (p, b) in &live {
        let s = String::from_utf8_lossy(b);
        assert!(
            !s.contains("/etc/wolos-forge"),
            "{}: the forge key path in the image",
            p.display()
        );
    }
    // The mok tier keeps making the machine's own key.
    let keys = read("files/usr/lib/wolos/boot-keys");
    assert!(
        keys.contains("openssl req -new -x509"),
        "boot-keys makes the machine's own key"
    );
}

#[test]
fn the_secure_boot_journey_drives_the_key_screen_to_the_live_system() {
    let j = read("tests/wolvm/journeys/live-secureboot.yaml");
    for want in [
        "--secureboot",
        "Security Violation",
        "Press any key to perform MOK management",
        ENROL,
        "SecureBoot enabled",
    ] {
        assert!(j.contains(want), "live-secureboot.yaml lacks {want}");
    }
}
