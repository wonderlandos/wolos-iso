// SPDX-License-Identifier: GPL-2.0-or-later
//! What a modern Intel or AMD laptop needs from the image (lead order 2026-11-04, "use advanced
//! drivers of Intel etc."): hardware video decode (VA-API), Vulkan, Wi-Fi/Bluetooth/audio
//! firmware, fingerprint reader, Thunderbolt/USB4, power and thermal daemons. Each need has a
//! package in `profile/packages.*` and a `wol doctor hardware` row; what is deliberately not on
//! the image is listed with its reason in docs/modules/hardware.md (wolos). Inventory and sizes:
//! wolos-iso README, section "Hardware support".
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf};

fn read(p: &str) -> String {
    let f = siblings::root().join(p);
    fs::read_to_string(&f).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// The package names of the two image lists (comments cut).
fn image() -> Vec<String> {
    let mut v = Vec::new();
    for f in ["profile/packages.x86_64", "profile/packages.desktop"] {
        for l in read(f).lines() {
            let l = l.split('#').next().unwrap().trim();
            v.extend(l.split_whitespace().map(String::from));
        }
    }
    v
}

/// Need -> packages that serve it (all must be on the image).
const NEEDS: &[(&str, &[&str])] = &[
    // Video decode: Intel Broadwell and newer through iHD, AMD (radeonsi) and Intel's
    // Vulkan video through mesa; libva-utils is what `wol doctor hardware` runs (vainfo).
    ("video decode", &["mesa", "intel-media-driver", "libva-utils"]),
    // Vulkan: anv (with hasvk for Haswell/Broadwell), RADV; the loader and the 32-bit pair for Steam.
    (
        "vulkan",
        &[
            "vulkan-intel",
            "vulkan-radeon",
            "vulkan-icd-loader",
            "vulkan-tools",
            "lib32-vulkan-intel",
            "lib32-vulkan-radeon",
        ],
    ),
    // Firmware: the meta package pulls every split package (Intel/Realtek/MediaTek/Atheros Wi-Fi
    // and Bluetooth, amdgpu, audio DSPs); microcode for both vendors; Sound Open Firmware.
    ("firmware", &["linux-firmware", "intel-ucode", "amd-ucode", "sof-firmware", "wireless-regdb"]),
    ("wifi and bluetooth", &["networkmanager", "iwd", "bluez", "bluez-utils"]),
    // Fingerprint reader: libfprint comes with fprintd (D-Bus activated, nothing runs without a reader).
    ("fingerprint", &["fprintd"]),
    // Thunderbolt / USB4: boltd authorises devices (kernel CONFIG_USB4 is in linux-wolos).
    ("thunderbolt", &["bolt"]),
    // Power: power-profiles-daemon is the one that Energy Mode drives; thermald is Intel's cooling helper.
    ("power and thermal", &["power-profiles-daemon", "thermald", "fwupd"]),
];

#[test]
fn the_image_ships_what_a_modern_laptop_needs() {
    let img = image();
    for (need, pkgs) in NEEDS {
        for p in *pkgs {
            assert!(
                img.iter().any(|i| i == p),
                "{need}: {p} is not on the image (profile/packages.x86_64, profile/packages.desktop)"
            );
        }
    }
}

/// Deliberately not on the image, each with the reason the hardware doc gives.
const NOT_SHIPPED: &[(&str, &str)] = &[
    ("xf86-video-intel", "Wayland only: the modesetting path of the kernel driver is faster and maintained"),
    ("xf86-video-amdgpu", "Wayland only"),
    ("xf86-video-ati", "Wayland only"),
    ("intel-compute-runtime", "OpenCL/Level Zero, 119 MiB, and 274 MiB more for its compiler: on demand, not on every disk"),
    ("intel-graphics-compiler", "the compiler of intel-compute-runtime, 274 MiB"),
    ("linux-firmware-qcom", "210 MiB for Snapdragon SoCs, which are not x86-64"),
    ("tlp", "a second power daemon: it fights power-profiles-daemon over the same sysfs files"),
    ("auto-cpufreq", "same reason as tlp"),
];

#[test]
fn what_is_left_off_is_off_for_a_written_reason() {
    let img = image();
    let doc = read("docs/modules/hardware.md");
    for (p, why) in NOT_SHIPPED {
        assert!(!img.iter().any(|i| i == p), "{p} is on the image: {why}");
        assert!(
            doc.contains(&format!("`{p}`")),
            "docs/modules/hardware.md does not say why `{p}` is not on the image ({why})"
        );
    }
}

#[test]
fn one_power_daemon_only() {
    let img = image();
    assert!(img.iter().any(|p| p == "power-profiles-daemon"));
    for other in ["tlp", "tlp-rdw", "auto-cpufreq", "laptop-mode-tools", "system76-power", "cpupower-gui"] {
        assert!(!img.iter().any(|p| p == other), "{other} would fight power-profiles-daemon");
    }
}

/// The `wol doctor hardware` rows: one per need above that a person can act on.
const ROWS: &[&str] = &[
    "hardware.video",
    "hardware.vulkan",
    "hardware.firmware",
    "hardware.fingerprint",
    "hardware.thunderbolt",
    "hardware.cpu-power",
    "hardware.cooling",
    "hardware.cpu-level",
];

#[test]
fn wol_doctor_hardware_has_a_row_for_each() {
    let code = read("crates/controller/src/doctor.rs") + &read("crates/controller/src/doctor/hw.rs");
    assert!(
        code.contains("\"sound\", \"games\", \"updates\", \"security\", \"storage\", \"network\", \"power\", \"display\",\n    \"hardware\"")
            || code.contains("\"display\", \"hardware\""),
        "AREAS lacks hardware"
    );
    let doc = read("docs/modules/doctor.md");
    let app = read("webapps/doctor/app.js");
    assert!(app.contains("\"display\", \"hardware\""), "webapps/doctor/app.js AREAS lacks hardware");
    for r in ROWS {
        let name = r.strip_prefix("hardware.").unwrap();
        assert!(code.contains(&format!("\"{name}\"")), "{r}: no check with this id in the doctor code");
        assert!(doc.contains(&format!("`{name}`")), "{r}: docs/modules/doctor.md does not list it");
    }
}

#[test]
fn the_readme_has_the_inventory() {
    let readme = read("README.md");
    assert!(readme.contains("Hardware support"), "README section 'Hardware support'");
    for p in ["fprintd", "bolt", "thermald", "intel-media-driver", "vulkan-radeon", "sof-firmware"] {
        assert!(readme.contains(p), "README inventory lacks {p}");
    }
}
