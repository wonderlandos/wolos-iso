// SPDX-License-Identifier: GPL-2.0-or-later
//! Hardware catalogue vs the Arch image: drivers the old boot scripts chose are still in the
//! catalogue (udev autoloads them by modalias), and every catalogue package ships offline
//! (`profile/packages.x86_64`, `profile/packages.desktop`) or is listed as not shipped.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf};
fn read(p: &str) -> String {
    let f = siblings::root()
        .join(p);
    fs::read_to_string(&f).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Catalogue packages deliberately not in the offline image.
const NOT_SHIPPED: &[(&str, &str)] = &[
    ("nvidia-open", "large; wolos-firstboot installs it online"),
    ("nvidia-utils", "comes with nvidia-open online"),
];

/// `linux-firmware` is a meta package over these split firmware packages.
const LINUX_FIRMWARE_PARTS: &[&str] = &[
    "linux-firmware-amdgpu",
    "linux-firmware-atheros",
    "linux-firmware-broadcom",
    "linux-firmware-intel",
    "linux-firmware-mediatek",
    "linux-firmware-nvidia",
    "linux-firmware-radeon",
    "linux-firmware-realtek",
];

/// The Arch packages the hardware catalogue names (wolos `crates/core/src/hardware/catalog.rs`,
/// a sibling: this crate is std only): the strings of its `FW_*` consts and of every `firmware:` /
/// `packages:` list.
fn catalogue_packages() -> Vec<String> {
    let src = read("crates/core/src/hardware/catalog.rs");
    let mut v: Vec<String> = Vec::new();
    for l in src.lines() {
        let l = l.trim();
        let list = if l.starts_with("const FW_") {
            l.split_once('=').map(|(_, r)| r)
        } else {
            l.strip_prefix("firmware:").or_else(|| l.strip_prefix("packages:"))
        };
        let Some(list) = list.and_then(|r| r.trim().strip_prefix("&[")) else { continue };
        let list = list.split(']').next().unwrap();
        for p in list.split(',').map(|p| p.trim().trim_matches('"')).filter(|p| !p.is_empty()) {
            if !v.iter().any(|x| x == p) {
                v.push(p.to_owned());
            }
        }
    }
    // The parse still sees the catalogue: a reformat that hides its lists fails here.
    for want in ["intel-ucode", "linux-firmware-intel", "vulkan-radeon", "nvidia-open"] {
        assert!(v.iter().any(|x| x == want), "catalog.rs: {want} not found ({v:?})");
    }
    v
}

fn shipped() -> Vec<String> {
    let mut v = Vec::new();
    for f in ["profile/packages.x86_64", "profile/packages.desktop"] {
        for l in read(f).lines() {
            let l = l.split('#').next().unwrap().trim();
            if !l.is_empty() {
                v.push(l.to_owned());
            }
        }
    }
    v
}

#[test]
fn every_catalogue_package_ships_offline_or_is_listed_as_not_shipped() {
    let pkgs = shipped();
    let has = |p: &str| pkgs.iter().any(|x| x == p);
    let covered = |p: &str| has(p) || (LINUX_FIRMWARE_PARTS.contains(&p) && has("linux-firmware"));
    for p in &catalogue_packages() {
        let p = p.as_str();
        let skip = NOT_SHIPPED.iter().any(|(n, _)| *n == p);
        assert!(covered(p) || skip, "{p} neither shipped nor in NOT_SHIPPED");
        assert!(
            !(covered(p) && skip),
            "{p} is shipped: drop it from NOT_SHIPPED"
        );
    }
    for want in [
        "linux-firmware",
        "intel-ucode",
        "amd-ucode",
        "sof-firmware",
        "vulkan-intel",
        "vulkan-radeon",
    ] {
        assert!(has(want), "{want}");
    }
    assert!(
        !pkgs.iter().any(|p| p.starts_with("nvidia-")),
        "NVIDIA stays an online install"
    );
}

/// An iPhone plugged in shows its photos and files, shares its internet and pairs; AirPlay
/// speakers show up as outputs. All from the official repos, nothing to set up.
#[test]
fn iphone_and_airplay_work_offline() {
    let pkgs = shipped();
    for p in [
        "usbmuxd",
        "libimobiledevice",
        "ifuse",
        "gvfs-afc",
        "libheif",
        "pipewire-zeroconf",
    ] {
        assert!(pkgs.iter().any(|x| x == p), "{p} ships offline");
    }
}

/// One sound server: JACK apps, cameras (V4L2 apps), GStreamer players and AirPlay from an
/// iPhone all go through PipeWire, so one volume and one device list cover everything.
#[test]
fn pipewire_carries_all_audio_and_video() {
    let pkgs = shipped();
    for p in [
        "pipewire-jack",
        "pipewire-v4l2",
        "pipewire-libcamera",
        "gst-plugin-pipewire",
        "shairport-sync",
    ] {
        assert!(pkgs.iter().any(|x| x == p), "{p} ships offline");
    }
    for p in ["jack2", "pulseaudio"] {
        assert!(
            !pkgs.iter().any(|x| x == p),
            "{p} would be a second sound server"
        );
    }
    let f = "files/usr/share/pipewire/pipewire.conf.d/71-wolos-airplay.conf";
    let conf = std::fs::read_to_string(format!("{}/{f}", siblings::root_str()))
        .unwrap_or_default();
    assert!(
        conf.contains("libpipewire-module-raop-discover"),
        "{f}: AirPlay speakers listed"
    );
}

/// The 0 dB rule's config half (docs/modules/audio.md): new microphones start at 40% instead of
/// WirePlumber's 100% = hardware maximum, the ALSA hardware mixer and dB table stay in charge,
/// realtime for the audio group. Settings' per-device overrides go to ~/.config, never here.
#[test]
fn microphones_do_not_start_at_hardware_max() {
    let conf = |f: &str| {
        std::fs::read_to_string(format!("{}/{f}", siblings::root_str()))
            .unwrap_or_else(|e| panic!("{f}: {e}"))
    };
    let input = conf("files/usr/share/wireplumber/wireplumber.conf.d/71-wolos-input.conf");
    assert!(
        input.lines().any(|l| l.trim() == "device.routes.default-source-volume = 0.064"),
        "inputs start at 40% (0.4^3 cubic)"
    );
    let alsa = conf("files/usr/share/wireplumber/wireplumber.conf.d/72-wolos-alsa.conf");
    assert!(alsa.lines().any(|l| l.trim() == "api.alsa.soft-mixer = false"));
    assert!(alsa.lines().any(|l| l.trim() == "api.alsa.ignore-dB = false"));
    let limits = conf("files/etc/security/limits.d/70-wolos-audio.conf");
    assert!(limits.lines().any(|l| l.trim() == "@audio - rtprio 95"));
    assert!(shipped().iter().any(|p| p == "rtkit"), "rtkit grants realtime to PipeWire clients");
}
