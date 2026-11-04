// SPDX-License-Identifier: GPL-2.0-or-later
//! Intel iGPU stack on first boot: firmware + user-space packages offline, i915 tunables, session env,
//! native-boot proof script. Evidence: docs/modules/intel-graphics.md.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn packages() -> Vec<String> {
    let mut v = Vec::new();
    for f in ["profile/packages.x86_64", "profile/packages.desktop"] {
        for l in read(f).lines() {
            let p = l.split('#').next().unwrap().trim();
            if !p.is_empty() {
                v.push(p.to_owned());
            }
        }
    }
    v
}

#[test]
fn image_has_intel_firmware_and_userspace() {
    let pkgs = packages();
    for want in [
        // i915/xe firmware (DMC, GuC, HuC) and Intel CPU microcode.
        "linux-firmware",
        "intel-ucode",
        // GL (iris) + Vulkan (anv) + VA-API (iHD), 64- and 32-bit for Steam.
        "mesa",
        "lib32-mesa",
        "vulkan-intel",
        "lib32-vulkan-intel",
        "intel-media-driver",
        // Proof tools for files/usr/lib/wolos/verify-gpu.sh.
        "mesa-utils",
        "vulkan-tools",
        "libva-utils",
        "glmark2",
        "vkmark",
    ] {
        assert!(pkgs.iter().any(|p| p == want), "missing package {want}");
    }
}
