// SPDX-License-Identifier: GPL-2.0-or-later
//! The WolOS installer on Arch (ADR 0016, ADR 0013): `install-run <disk>` from the Install WolOS app
//! (or `wolos.autoinstall=<disk>` on the kernel command line) erases only the picked disk, makes a
//! GPT with an ESP and one btrfs partition, subvolumes `@wolos-a` `@home` `@var` (the first update
//! makes `@wolos-b`), copies the live root into slot a, and boots it with systemd-boot and boot
//! counting. Every tool is a fake that logs its arguments: none may ever name another disk.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn repo(p: &str) -> PathBuf {
    siblings::root()
        .join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn lib(name: &str) -> PathBuf {
    repo(&format!("files/usr/lib/wolos/{name}"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-install-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("bin")).unwrap();
    d
}

fn fake(d: &Path, name: &str, body: &str) {
    let p = d.join("bin").join(name);
    let log = d.join("log");
    fs::write(
        &p,
        format!(
            "#!/bin/sh\necho \"{name} $*\" >> {}\n{body}\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
}

fn log(d: &Path) -> String {
    fs::read_to_string(d.join("log")).unwrap_or_default()
}

fn path(d: &Path) -> String {
    format!("{}:/usr/bin:/bin", d.join("bin").display())
}

/// lsblk -dnpbo NAME,SIZE,TYPE,RO: three disks, a loop device, a CD, zram; the live medium is sdb.
const LSBLK: &str = "/dev/nvme0n1 512110190592 disk 0\n/dev/sda 1000204886016 disk 0\n\
/dev/sdb 32010928128 disk 0\n/dev/loop0 900000000 loop 1\n/dev/sr0 1000000 rom 0\n\
/dev/zram0 8000000000 disk 0\n/dev/sdc 64000000000 disk 1\n";

fn disks_fakes(d: &Path) {
    fake(
        d,
        "lsblk",
        &format!(
            "case \"$*\" in\n*-dnpbo*) printf '{}' ;;\n*PKNAME*) echo /dev/sdb ;;\n\
             *PARTTYPE*) cat \"$WOLOS_TEST_PARTS\" 2>/dev/null ;;\nesac",
            LSBLK.replace('\n', "\\n")
        ),
    );
    fake(
        d,
        "findmnt",
        "case \"$*\" in\n*bootmnt*) echo /dev/sdb1 ;;\n*SOURCE*) cat \"$WOLOS_TEST_MOUNTS\" 2>/dev/null ;;\nesac",
    );
}

/// A full install against fakes: target root in a temp dir, mount/umount do nothing.
fn install(d: &Path, disk: &str) -> Output {
    install_with(d, disk, "off")
}

/// Tool fakes that leave the file a later step expects.
const OPENSSL: &str =
    "while [ $# -gt 0 ]; do case $1 in -keyout|-out) echo x > \"$2\"; shift ;; esac; shift; done";

const SBSIGN: &str =
    "for a; do case $a in --output=*) echo signed > \"${a#--output=}\" ;; esac; done";

const UKIFY: &str =
    "for a; do case $a in --output=*) echo image > \"${a#--output=}\" ;; esac; done";

fn install_with(d: &Path, disk: &str, unlock: &str) -> Output {
    disks_fakes(d);
    let t = d.join("target");
    fs::create_dir_all(&t).unwrap();
    for (n, body) in [
        ("systemd-repart", ""),
        ("udevadm", ""),
        ("mount", ""),
        ("umount", ""),
        ("btrfs", ""),
        ("sync", ""),
        ("cryptsetup", ""),
        (
            "wol",
            "case \"$*\" in\n\"encrypt decide\"*) [ \"$WOLOS_TEST_UNLOCK\" = refuse ] && exit 1; echo ${WOLOS_TEST_UNLOCK:-off} ;;\n\
             \"encrypt words\"*) echo \"rd.luks.name=$3=wolosroot root=/dev/mapper/wolosroot\" ;;\n\
             \"boot detect\"*) echo off ;;\n\"boot key-screen\"*) echo no ;;\n\"boot note\"*) echo note ;;\nesac",
        ),
        ("userdel", ""),
        (
            "bootctl",
            "mkdir -p \"${2#--esp-path=}/EFI/systemd\" \"${2#--esp-path=}/EFI/BOOT\"",
        ),
        ("openssl", OPENSSL),
        ("sbsign-fake", SBSIGN),
        ("ukify", UKIFY),
        (
            "blkid",
            "case \"$*\" in *nvme0n1p1*) echo AAAA-BBBB ;; *) echo 1111-2222 ;; esac",
        ),
        (
            "unsquashfs",
            // The live root: modules with the kernel, live-only files, archiso bits.
            "R=$3\nmkdir -p $R/var/lib/pacman/local $R/usr/lib/modules/7.3.0-wolos $R/etc/wolos $R/etc/sysusers.d $R/etc/sudoers.d \
             $R/etc/mkinitcpio.conf.d $R/etc/mkinitcpio.d $R/boot $R/home/live\n\
             echo kernel > $R/usr/lib/modules/7.3.0-wolos/vmlinuz\necho linux-wolos > $R/usr/lib/modules/7.3.0-wolos/pkgbase\n\
             touch $R/etc/sysusers.d/wolos-live.conf $R/etc/sudoers.d/wolos-live $R/etc/wolos/login.json $R/etc/wolos-live \
             $R/etc/mkinitcpio.conf.d/archiso.conf\necho live > $R/etc/wolos/shell.json\necho pkg > $R/etc/wolos/shell.json.pacnew\n\
             echo 2026-10-20 > $R/etc/wolos/snapshot\nprintf '[options]\\nHoldPkg = pacman\\n' > $R/etc/pacman.conf",
        ),
        (
            "arch-chroot",
            "case \"$*\" in *mkinitcpio*) echo initrd > \"$1/boot/initramfs-linux-wolos.img\" ;; esac",
        ),
    ] {
        fake(d, n, body);
    }
    Command::new("sh")
        .arg(lib("install-run"))
        .arg(disk)
        .env("WOLOS_INSTALL_PATH", path(d))
        .env("WOLOS_INSTALL_ROOT", &t)
        .env("WOLOS_SOURCE_SFS", d.join("airootfs.sfs"))
        .env("WOLOS_TEST_PARTS", d.join("parts"))
        .env("WOLOS_TEST_MOUNTS", d.join("mounts"))
        .env("WOLOS_EFIVARS", d.join("efivars"))
        .env("WOLOS_NO_INHIBIT", "1")
        .env("WOLOS_TEST_UNLOCK", unlock)
        .env("WOLOS_KEYDIR", d.join("keys"))
        .env("WOLOS_SB_RUN", d.join("sbrun"))
        .env("WOLOS_TPM_DIR", d.join("no-tpm"))
        .env("WOLOS_BOOT_NOTE", d.join("note"))
        .env("WOLOS_SBSIGN", d.join("bin/sbsign-fake"))
        .output()
        .unwrap()
}

/// The pacman database lives in the slot (/usr), not in the shared @var: each slot knows its own
/// packages. The live root keeps pacman's default (/var/lib/pacman, where pacstrap writes, and
/// pacman resolves DBPath on the build host); install-run moves it and names it in pacman.conf.
#[test]
fn pacman_database_stays_in_the_slot() {
    assert!(read("profile/pacman.conf").contains("\nDBPath = /usr/lib/pacman/\n"));
    assert!(!read("profile/airootfs/etc/pacman.conf").contains("\nDBPath"));
    assert!(read("files/usr/lib/wolos/install-run").contains("DBPath = /usr/lib/pacman/"));
}

/// Unattended VM install: `wolos.autoinstall=<disk>` on the live command line.
#[test]
fn autoinstall_from_the_kernel_command_line() {
    let u = read("profile/airootfs/etc/systemd/system/wolos-autoinstall.service");
    assert!(u.contains("ConditionKernelCommandLine=wolos.autoinstall"));
    assert!(u.contains("ExecStart=/usr/lib/wolos/autoinstall"));
    assert!(repo("profile/airootfs/etc/systemd/system/multi-user.target.wants/wolos-autoinstall.service")
        .symlink_metadata()
        .is_ok());
    let d = scratch("auto");
    fake(&d, "systemctl", "");
    fake(&d, "install-run", "");
    fs::write(
        d.join("cmdline"),
        "quiet splash wolos.autoinstall=/dev/vda console=ttyS0\n",
    )
    .unwrap();
    let st = Command::new("sh")
        .arg(repo("files/usr/lib/wolos/autoinstall"))
        .env("PATH", path(&d))
        .env("WOLOS_CMDLINE", d.join("cmdline"))
        .env("WOLOS_INSTALL_RUN", d.join("bin/install-run"))
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(log(&d), "install-run /dev/vda\nsystemctl poweroff\n");
}
