// SPDX-License-Identifier: GPL-2.0-or-later
//! The Arch Linux image (ADR 0016): mkarchiso profile `profile`, pinned to one Arch Linux Archive
//! date, `[wolos]` before the Arch repos, linux-wolos only, the `[wolos]` repo build and the release
//! manifest. Scripts run against fakes; nothing here needs root or the network.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn repo(p: &str) -> PathBuf {
    siblings::root()
        .join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wol-arch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn bash_n(p: &str) {
    let o = Command::new("bash")
        .arg("-n")
        .arg(repo(p))
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{p}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn snapshot() -> String {
    read("profile/snapshot").trim().to_string()
}

fn packages() -> Vec<String> {
    let mut v = Vec::new();
    for f in ["profile/packages.x86_64", "profile/packages.desktop"] {
        for l in read(f).lines() {
            let l = l.split('#').next().unwrap().trim();
            v.extend(l.split_whitespace().map(String::from));
        }
    }
    v
}

#[test]
fn snapshot_is_one_archive_date() {
    let s = snapshot();
    let parts: Vec<&str> = s.split('-').collect();
    assert!(
        parts.len() == 3
            && [4, 2, 2]
                .iter()
                .zip(&parts)
                .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_digit())),
        "snapshot is YYYY-MM-DD: {s:?}"
    );
}

/// pacman reads only the snapshot date; `[wolos]` comes first so linux-wolos and the WolOS
/// packages win over same-named Arch ones. Arch servers inline (nothing rolls in between
/// releases); `[wolos]` and `[wolos-cachy]` read the dated wolos-mirrorlist (ADR 0019).
#[test]
fn pacman_conf_is_pinned_and_wolos_comes_first() {
    let date = snapshot().replace('-', "/");
    for f in [
        "profile/pacman.conf",
        "profile/airootfs/etc/pacman.conf",
    ] {
        let c = read(f);
        let repos: Vec<&str> = c
            .lines()
            .filter(|l| l.starts_with('[') && *l != "[options]")
            .collect();
        assert_eq!(repos, ["[wolos]", "[core]", "[extra]", "[multilib]"], "{f}");
        let servers: Vec<&str> = c
            .lines()
            .filter_map(|l| l.strip_prefix("Server = "))
            .collect();
        assert_eq!(servers.len(), 3, "{f}");
        for s in &servers {
            assert_eq!(
                *s,
                format!("https://archive.archlinux.org/repos/{date}/$repo/os/$arch"),
                "{f}"
            );
        }
        let inc: Vec<&str> = c.lines().filter(|l| l.contains("Include")).collect();
        assert_eq!(
            inc,
            [
                "Include = /etc/pacman.d/wolos-mirrorlist",
                "#Include = /etc/pacman.d/wolos-mirrorlist"
            ],
            "{f}: only [wolos] and the gated [wolos-cachy]"
        );
        assert!(c.contains("SigLevel = Required"), "{f}: signed packages");
    }
}

/// `wol install` runs real tools that its tests fake: each one has to reach the image. A tool that
/// isn't listed here fails, so a new one can't arrive without its package.
#[test]
fn every_tool_wol_install_runs_ships_in_the_image() {
    const OWNERS: &[(&str, &[&str])] = &[
        ("arch-install-scripts", &["arch-chroot"]),
        ("btrfs-progs", &["btrfs", "mkfs.btrfs"]),
        ("dosfstools", &["mkfs.vfat"]),
        (
            "e2fsprogs",
            &["dumpe2fs", "e2fsck", "mkfs.ext4", "resize2fs"],
        ),
        ("ntfsprogs", &["ntfsresize"]),
        ("xfsprogs", &["mkfs.xfs", "xfs_growfs"]),
        // base: coreutils, systemd, util-linux.
        (
            "wolos-base",
            &[
                "blkid", "cat", "df", "lsblk", "mkdir", "mkswap", "mount", "partx", "sfdisk",
                "udevadm", "umount",
            ],
        ),
    ];
    let source = read("crates/controller/src/install.rs");
    let live = source.split("#[cfg(test)]").next().unwrap();
    let live = live.replace("= [\"", "&[\"");
    let listed = packages();
    let mut seen = 0;
    for call in live.split("&[\"").skip(1) {
        let tool = call.split('"').next().unwrap();
        let owner = OWNERS.iter().find(|(_, tools)| tools.contains(&tool));
        let (package, _) = owner.unwrap_or_else(|| panic!("{tool}: which package ships it?"));
        assert!(
            listed.iter().any(|p| p == package),
            "{tool} needs {package}"
        );
        seen += 1;
    }
    assert!(seen > 20, "the scan found {seen} tool calls");
}

#[test]
fn packages_are_arch_names_with_linux_wolos_only() {
    let p = packages();
    for want in [
        "wolos-base",
        "linux-firmware",
        "intel-ucode",
        "amd-ucode",
        "mkinitcpio-archiso",
        "btrfs-progs",
        "dosfstools",
        "plymouth",
        "nftables",
        "hyprland-wolos",
        "mesa",
        "vulkan-intel",
        "vulkan-radeon",
        "lib32-mesa",
        "intel-media-driver",
        "pipewire",
        "wireplumber",
        "networkmanager",
        "bluez",
        "udisks2",
        "steam",
        "helium-browser",
        "inter-font",
        "adw-gtk-theme",
    ] {
        assert!(p.iter().any(|x| x == want), "missing package {want}");
    }
    // The base parts come only through wolos-base (ADR 0017), never listed one by one.
    for gone in [
        "linux",
        "base",
        "linux-wolos",
        "sudo",
        "coreutils",
        "zsh",
        "pkg",
        "drm-kmod",
        "linux_base-rl9",
        "linux-steam-utils",
        "bsddialog",
        "webcamd",
        "networkmgr",
        "hyprland",
        "pipewire-spa-oss",
        "fusefs-ntfs",
        "nvidia-open",
        "firefox",
    ] {
        assert!(!p.iter().any(|x| x == gone), "{gone} is not shipped");
    }
    assert!(
        p.iter().all(|x| x
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_+.@".contains(c))),
        "pacman package names only"
    );
    let mut sorted = p.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), p.len(), "no package listed twice");
}

#[test]
fn profile_boots_linux_wolos_quietly_with_an_install_entry() {
    bash_n("profile/profiledef.sh");
    let d = read("profile/profiledef.sh");
    for want in [
        "iso_name=\"wolos\"",
        "bootmodes=('uefi.systemd-boot')",
        "pacman_conf=\"pacman.conf\"",
    ] {
        assert!(d.contains(want), "profiledef.sh lacks {want}");
    }
    let live = read("profile/efiboot/loader/entries/01-wolos.conf");
    let try_it = read("profile/efiboot/loader/entries/02-wolos-try.conf");
    for e in [&live, &try_it] {
        assert!(e.contains("vmlinuz-linux-wolos") && e.contains("initramfs-linux-wolos.img"));
        assert!(e.contains(" quiet splash ") && e.contains("loglevel=3"));
        // No audit userspace and no SELinux/AppArmor: the kernel audit stays off.
        assert!(e.contains(" audit=0 "), "{e}");
    }
    assert!(try_it.contains(" wolos.try") && !live.contains("wolos.try"));
    // The first screen is the light installer session; the desktop starts only when asked.
    let first = read("profile/airootfs/etc/systemd/system/wolos-installer.service");
    assert!(
        first.contains("ConditionKernelCommandLine=!wolos.try"),
        "{first}"
    );
    assert!(first.contains("ExecStart=/usr/lib/wolos/installer-session"));
    assert!(
        repo(
            "profile/airootfs/etc/systemd/system/multi-user.target.wants/wolos-installer.service"
        )
        .is_symlink()
    );
    let wait = read("profile/airootfs/etc/systemd/system/wold.service.d/wolos-live.conf");
    assert!(
        wait.contains("|/run/wolos/try") && wait.contains("|wolos.try"),
        "{wait}"
    );
    let session = read("files/usr/lib/wolos/installer-session");
    assert!(
        session.contains("hypr/installer.conf")
            && session.contains("start --no-block wold.service")
    );
    let conf = read("desktop/overlay/usr/share/wolos/hypr/installer.conf");
    assert!(conf.contains("wol-installer --boot") && !conf.contains("wol-shell"));
    assert!(read("profile/efiboot/loader/loader.conf").contains("timeout 0"));
    let preset = read("profile/airootfs/etc/mkinitcpio.d/linux-wolos.preset");
    assert!(preset.contains("/boot/vmlinuz-linux-wolos") && preset.contains("archiso"));
}

/// WOLOS_STAGE_ONLY=1: the profile is staged (desktop packages merged, [wolos] read from the local
/// repo for the build, image files, overlay and design in airootfs) and mkarchiso is not run.
#[test]
fn build_stages_the_profile_from_the_local_repo() {
    bash_n("build.sh");
    let d = scratch("stage");
    let local = d.join("repo/x86_64");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("wolos.db"), "").unwrap();
    let o = Command::new("bash")
        .arg(repo("build.sh"))
        .env("WOLOS_WORK", &d)
        .env("WOLOS_REPO_DIR", d.join("repo"))
        .env("WOLOS_STAGE_ONLY", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let p = d.join("profile");
    let pk = fs::read_to_string(p.join("packages.x86_64")).unwrap();
    assert!(
        pk.contains("wolos-base\n") && pk.contains("steam\n"),
        "{pk}"
    );
    let pc = fs::read_to_string(p.join("pacman.conf")).unwrap();
    assert!(
        pc.contains(&format!(
            "Server = file://{}/$arch",
            d.join("repo").display()
        )),
        "{pc}"
    );
    // pacstrap writes the database where pacman puts it on any host (install-run moves it into
    // the slot): the build's pacman.conf has no DBPath, the installed system's keeps it.
    assert!(!pc.contains("DBPath"), "{pc}");
    // No systemd-firstboot prompt on the console: the Setup Assistant asks instead.
    let fb =
        std::fs::read_link(d.join("profile/airootfs/etc/systemd/system/systemd-firstboot.service"));
    assert_eq!(fb.unwrap(), PathBuf::from("/dev/null"));
    // The CI builder has the tree without .git: the revision comes from the repo, or "local".
    assert!(read("build.sh").contains("rev-parse --short HEAD 2>/dev/null || echo local"));
    let wolos = pc
        .split("\n[wolos]\n")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    assert!(
        wolos.contains("SigLevel = Never") && wolos.matches("SigLevel").count() == 1,
        "{wolos}"
    );
    // The installed system's pacman.conf keeps the published [wolos] server.
    let ac = fs::read_to_string(p.join("airootfs/etc/pacman.conf")).unwrap();
    assert!(!ac.contains("file://"));
    assert!(
        !ac.contains("\nDBPath"),
        "install-run names it on the installed system"
    );
    assert!(!d.join("profile/airootfs/usr/lib/pacman").exists());
    // Live-only files; everything else comes from the wolos-system package (a file in both would
    // stop pacstrap: airootfs is copied first).
    for f in [
        "airootfs/etc/wolos/snapshot",
        "airootfs/etc/sysusers.d/wolos-live.conf",
        "airootfs/etc/wolos/login.json",
        "airootfs/etc/sudoers.d/wolos-live",
    ] {
        assert!(p.join(f).exists(), "{f} not staged");
    }
    assert_eq!(
        fs::read_to_string(p.join("airootfs/etc/wolos/snapshot"))
            .unwrap()
            .trim(),
        snapshot()
    );
    // The mark gives the live dock "Install WolOS" (apps::pins); shell.json stays the package's.
    assert!(p.join("airootfs/etc/wolos-live").exists());
    assert!(!p.join("airootfs/etc/wolos/shell.json").exists());
    assert!(
        !p.join("airootfs/usr/lib/systemd/system/wold.service")
            .exists()
    );
    assert!(!p.join("airootfs/usr/bin").exists());
    let sudo = fs::read_to_string(p.join("airootfs/etc/sudoers.d/wolos-live")).unwrap();
    assert_eq!(
        sudo.trim(),
        "live ALL=(root) NOPASSWD: /usr/lib/wolos/install-run, /usr/lib/wolos/install-check, /usr/bin/systemctl reboot, /usr/bin/systemctl poweroff"
    );
}

/// The release manifest: snapshot date, repo revision, image URL and its sha256.
#[test]
fn release_manifest_names_snapshot_repo_image_and_hash() {
    bash_n("release/release-manifest.sh");
    let d = scratch("manifest");
    let iso = d.join("wolos-0.11-x86_64.iso");
    fs::write(&iso, "iso bytes").unwrap();
    let o = Command::new("bash")
        .arg(repo("release/release-manifest.sh"))
        .arg(&iso)
        .arg("2026-10-20")
        .arg("r42")
        .arg("https://example.org/img")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["snapshot"], "2026/10/20");
    assert_eq!(v["wolos_repo"], "r42");
    assert_eq!(v["image"], "https://example.org/img/wolos-0.11-x86_64.iso");
    // sha256("iso bytes")
    let sum = Command::new("sha256sum").arg(&iso).output().unwrap();
    let sum = String::from_utf8_lossy(&sum.stdout);
    assert_eq!(v["sha256"], sum.split_whitespace().next().unwrap());
}

/// base-manifest.sh PKG...: "sha256  /path" for every file of the base packages, sorted; the
/// package metadata files are not system files.
#[test]
fn base_manifest_lists_every_file_with_its_hash() {
    let d = scratch("basemanifest");
    let root = d.join("root");
    fs::create_dir_all(root.join("usr/bin")).unwrap();
    fs::write(root.join("usr/bin/ls"), "ls").unwrap();
    fs::write(root.join(".PKGINFO"), "pkgname = x").unwrap();
    let pkg = d.join("x-1-1-x86_64.pkg.tar.zst");
    let st = Command::new("bsdtar")
        .arg("--zstd")
        .arg("-cf")
        .arg(&pkg)
        .arg("-C")
        .arg(&root)
        .arg(".PKGINFO")
        .arg("usr")
        .status()
        .unwrap();
    assert!(st.success());
    let o = Command::new("bash")
        .arg(repo("wolos-base/base-manifest.sh"))
        .arg(&pkg)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    let sum = Command::new("sha256sum")
        .arg(root.join("usr/bin/ls"))
        .output()
        .unwrap();
    let sum = String::from_utf8_lossy(&sum.stdout);
    assert_eq!(
        out,
        format!("{}  /usr/bin/ls\n", sum.split_whitespace().next().unwrap())
    );
    // shadow's newuidmap carries a file capability: restoring it needs root, the builder is not.
    assert!(read("wolos-base/base-manifest.sh").contains("--no-xattrs"));
}

/// wolos-system installs next to the Arch base: a file another package owns makes pacman refuse
/// it, unless wolos-system replaces that package.
#[test]
fn wolos_system_files_do_not_collide_with_arch_packages() {
    // Paths Arch packages own (checked against core.files and extra.files of the snapshot).
    for (path, owner) in [
        ("etc/nftables.conf", "nftables"),
        ("etc/xdg/foot/foot.ini", "foot"),
        ("usr/share/zsh/scripts/newuser", "zsh"),
        ("etc/zsh/zshrc", "grml-zsh-config"),
        ("etc/zsh/zprofile", "zsh"),
        ("usr/share/applications/steam.desktop", "steam"),
    ] {
        for tree in ["desktop/overlay", "files"] {
            assert!(
                !repo(&format!("{tree}/{path}")).exists(),
                "{path} is {owner}'s"
            );
        }
    }
    let p = read("wolos-system/PKGBUILD");
    assert!(
        p.contains("provides=(default-cursors)"),
        "default-cursors owns icons/default"
    );
    assert!(p.contains("conflicts=(default-cursors)"));
    assert!(p.contains("etc/wolos/nftables.conf") && !p.contains(" etc/nftables.conf"));
    // os-release is wolos-base's (its hook names the system); wolos-system leaves it out.
    assert!(p.contains("rm \"$pkgdir/usr/share/wolos/os-release\""));
    // Our Steam entry replaces the steam package's through a hook, as os-release does.
    let h = read("files/usr/share/libalpm/hooks/wolos-steam-entry.hook");
    assert!(
        h.contains("Target = usr/share/applications/steam.desktop"),
        "{h}"
    );
    assert!(
        h.contains("/usr/share/wolos/applications/steam.desktop"),
        "{h}"
    );
}

#[test]
fn live_iso_secure_boot_layout_is_ready_and_off() {
    // shim first (the pinned build), systemd-boot second, one UKI: behind WOLOS_SB_LAYOUT, off.
    let b = read("build.sh");
    assert!(
        b.contains(r#"if [ "${WOLOS_SB_LAYOUT:-0}" = 1 ]; then"#),
        "no switch"
    );
    assert!(b.contains(r#""$IMG/sb-layout.sh" layout"#));
    let l = read("release/sb-layout.sh");
    let shim = read("wolos-shim/PKGBUILD");
    for f in ["shimx64.efi", "mmx64.efi"] {
        let pin = shim
            .lines()
            .find(|x| x.contains(f) && x.contains("sha256sum -c"))
            .unwrap();
        let sha = pin
            .split('"')
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            l.contains(sha),
            "sb-layout.sh does not pin {f} as wolos-shim does"
        );
    }
    for want in [
        "::/EFI/BOOT/BOOTX64.EFI",
        "::/EFI/BOOT/grubx64.efi",
        "::/EFI/BOOT/mmx64.efi",
        "::/EFI/Linux/wolos.efi",
        "::/wolos-secureboot.cer",
        "--join-profile",
    ] {
        assert!(l.contains(want), "sb-layout.sh lacks {want}");
    }
    // release.yml signs and boots it only with both secrets; the switch is a repository variable.
    let w = read(".github/workflows/release.yml");
    for want in [
        "WOLOS_SB_LAYOUT: ${{ vars.WOLOS_SB_LAYOUT || '0' }}",
        "SB_KEY: ${{ secrets.WOLOS_SB_KEY }}",
        r#"if [ -n "$SB_KEY" ] && [ -n "$SB_CERT" ]; then"#,
        "release/sb-layout.sh sign",
        "tests/test-secureboot.sh",
    ] {
        assert!(w.contains(want), "release.yml lacks {want}");
    }
    // Without the certificate the Secure Boot test reports a skip and passes.
    let out = Command::new("bash")
        .arg(repo("tests/test-secureboot.sh"))
        .arg("/nonexistent.iso")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("SKIP"));
    // The owner's steps to make the key are written down.
    let adr = read("docs/adr/0025-signed-boot.md");
    assert!(adr.contains("gh secret set WOLOS_SB_KEY") && adr.contains("openssl req -new -x509"));
}

#[test]
fn vm_guest_tools_start_only_in_their_hypervisor() {
    let pkgs = read("profile/packages.x86_64");
    for p in [
        "virtualbox-guest-utils",
        "open-vm-tools",
        "spice-vdagent",
        "qemu-guest-agent",
    ] {
        assert!(pkgs.lines().any(|l| l.trim() == p), "{p} not in the image");
    }
    // The units gate themselves (ConditionVirtualization=oracle / vmware): enabled everywhere.
    let preset = read("files/usr/lib/systemd/system-preset/50-wolos.preset");
    let wants = "profile/airootfs/etc/systemd/system/multi-user.target.wants";
    for u in ["vboxservice.service", "vmtoolsd.service"] {
        assert!(
            preset.lines().any(|l| l == format!("enable {u}")),
            "{u} not preset"
        );
        let l = fs::read_link(repo(wants).join(u)).expect("live symlink");
        assert_eq!(l, Path::new("/usr/lib/systemd/system").join(u));
    }
    // VBoxClient --vmsvga only under VirtualBox, in every Hyprland session.
    for c in ["hyprland", "installer", "setup", "login"] {
        let s = read(&format!("desktop/overlay/usr/share/wolos/hypr/{c}.conf"));
        assert!(
            s.contains(r#"[ "$(systemd-detect-virt)" = oracle ] && exec VBoxClient --vmsvga"#),
            "{c}.conf"
        );
    }
    // The kernel has the VirtualBox modules: no Arch `linux` pulled in for them.
    assert!(read("wolos-base/PKGBUILD").contains("'VIRTUALBOX-GUEST-MODULES'"));
}

#[test]
fn the_work_dir_is_removed_without_crossing_mounts() {
    // A killed mkarchiso left pacstrap's devtmpfs mounted in the work dir; a plain rm -rf then
    // deleted the host's device nodes. Mounts go first, and rm stays on one file system.
    let b = read("build.sh");
    assert!(b.contains(r#"rm -rf --one-file-system "$WORK/work""#));
    assert!(b.contains(r#"umount -l "$m""#));
    let plain = b
        .lines()
        .filter(|l| {
            l.contains("rm -rf") && l.contains("$WORK/work") && !l.contains("--one-file-system")
        })
        .count();
    assert_eq!(
        plain, 0,
        "a plain rm -rf of the work dir is left in build.sh"
    );
}

#[test]
fn iso_fits_one_release_asset() {
    // Release assets max out at 2 GiB: xz with the x86 filter packs binaries tighter than zstd,
    // and GPU drivers load from the squashfs after switch_root instead of from the initramfs.
    let p = read("profile/profiledef.sh");
    assert!(p.contains("'-comp' 'xz' '-Xbcj' 'x86'"), "{p}");
    let hooks = read("profile/airootfs/etc/mkinitcpio.conf.d/archiso.conf");
    let hooks = hooks.lines().find(|l| l.starts_with("HOOKS=")).unwrap();
    assert!(!hooks.split(['(', ' ', ')']).any(|h| h == "kms"), "{hooks}");
    // The headers' vmlinux loses its DWARF (BTF stays), and the build stops on an oversized ISO.
    let k = read("linux-wolos/PKGBUILD");
    assert!(
        k.contains(r#"strip -v --strip-debug "$builddir/vmlinux""#),
        "vmlinux not stripped"
    );
    let b = read("build.sh");
    assert!(
        b.contains("$((2047 * 1024 * 1024))"),
        "no ISO size limit in build.sh"
    );
}

/// Helium (the default browser) is the official x86_64 tarball repackaged: pinned version and
/// checksum, signature checked against the release key pinned by fingerprint, Wayland wrapper.
#[test]
fn helium_browser_is_the_signed_official_tarball_repackaged() {
    let pkg = read("helium-browser/PKGBUILD");
    bash_n("helium-browser/PKGBUILD");
    bash_n("helium-browser/helium.sh");
    assert!(pkg.contains("pkgname=helium-browser"));
    assert!(pkg.contains("pkgver=0.20.2.1"));
    assert!(pkg.contains("helium-${pkgver}-x86_64_linux.tar.xz"));
    assert!(pkg.contains(".tar.xz.asc"));
    // The release key (docs: README "Signature"), by fingerprint; its file is makepkg's keys/pgp.
    let fpr = "BE677C1989D35EAB2C5F26C9351601AD01D6378E";
    assert!(
        pkg.contains(&format!("validpgpkeys=('{fpr}'")),
        "validpgpkeys"
    );
    let key = read(&format!("helium-browser/keys/pgp/{fpr}.asc"));
    assert!(key.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----"));
    // sha256sums: the tarball is pinned (first), the .asc is checked by gpg (SKIP), and the two
    // files beside the recipe carry their hash.
    let sums = pkg
        .split("sha256sums=(")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    let hashes: Vec<&str> = sums
        .split_whitespace()
        .map(|w| w.trim_matches('\''))
        .collect();
    assert_eq!(hashes.len(), 4, "{sums}");
    assert_eq!(
        hashes[0],
        "ba92acb27ec6cda36a8b948e2154009aaee0d633222ad244be25e736a143e6fa"
    );
    assert_eq!(hashes[1], "SKIP", "the .asc is verified, not hashed");
    for h in [hashes[0], hashes[2], hashes[3]] {
        assert!(
            h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()),
            "{h}"
        );
    }
    for want in [
        "/opt/helium",
        "/usr/bin/helium",
        "helium.desktop",
        "hicolor",
    ] {
        assert!(pkg.contains(want), "PKGBUILD lacks {want}");
    }
    // Wrapper: Wayland by default, Wayland app-id `helium` (the Dock's running dot), VA-API only
    // with a render node, per-user flags file.
    let w = read("helium-browser/helium.sh");
    for want in [
        "--ozone-platform-hint=auto",
        "CHROME_DESKTOP=helium.desktop",
        "/dev/dri/renderD",
        "AcceleratedVideoDecodeLinuxGL",
        "exec /opt/helium/helium-wrapper",
        "helium-flags.conf",
    ] {
        assert!(w.contains(want), "helium.sh lacks {want}");
    }
    // Built with the other [wolos] packages, rebuilt when its recipe changes.
    let list = read("packages");
    assert!(
        list.lines()
            .any(|l| l.starts_with("helium-browser ") && l.contains("helium-browser/"))
    );
}
