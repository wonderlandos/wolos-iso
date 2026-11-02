// SPDX-License-Identifier: GPL-2.0-or-later
//! Wine/Proton and PipeWire defaults ship as plain config values (ported from the CachyOS and
//! Bazzite settings): kernel limits, file-descriptor limits, realtime audio, Bluetooth codecs.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::path::PathBuf;
fn read(p: &str) -> String {
    let f = siblings::root()
        .join(p);
    std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn end_user_image_has_no_toolchain_or_gnu_docs() {
    let all = format!(
        "{}\n{}\n{}",
        read("profile/packages.desktop"),
        read("profile/packages.x86_64"),
        read("profile/packages-base.txt")
    );
    for banned in [
        "gcc",
        "texinfo",
        "man-pages",
        "base-devel",
        "make",
        "autoconf",
    ] {
        assert!(
            !all.lines()
                .any(|l| l.split_whitespace().next() == Some(banned)),
            "{banned} ships on the image"
        );
    }
}

#[test]
fn gamemode_and_rtkit_are_installed() {
    let p = read("profile/packages.desktop");
    for want in ["gamemode", "lib32-gamemode", "rtkit"] {
        assert!(
            p.lines().any(|l| l.split_whitespace().next() == Some(want)),
            "{want}"
        );
    }
}

/// "Reduce background noise" (Settings > Sound) loads the RNNoise LADSPA plugin on demand: the
/// plugin ships offline, the module does not load statically.
#[test]
fn noise_suppression_plugin_ships_and_nothing_loads_it_statically() {
    let p = read("profile/packages.desktop");
    assert!(
        p.lines()
            .any(|l| l.split_whitespace().next() == Some("noise-suppression-for-voice")),
        "noise-suppression-for-voice"
    );
    for f in [
        "files/usr/share/pipewire/pipewire.conf.d/70-wolos-latency.conf",
        "files/usr/share/pipewire/pipewire.conf.d/71-wolos-airplay.conf",
        "files/usr/share/pipewire/pipewire-pulse.conf.d/70-wolos-latency.conf",
        "files/usr/share/wireplumber/wireplumber.conf.d/70-wolos-bluetooth.conf",
        "files/usr/share/wireplumber/wireplumber.conf.d/71-wolos-input.conf",
        "files/usr/share/wireplumber/wireplumber.conf.d/72-wolos-alsa.conf",
    ] {
        let t = read(f);
        for banned in ["module-echo-cancel", "module-loopback", "filter-chain", "rnnoise"] {
            assert!(!t.contains(banned), "{f} loads {banned} statically");
        }
    }
}

/// The pulse floor on the image is 256 frames; only games (Wine, Steam, gamescope) may ask for
/// 64, a VM gets 1024 (pipewire-pulse.conf.d/70-wolos-latency.conf).
#[test]
fn pulse_clients_get_a_256_frame_floor_games_64() {
    let t = read("files/usr/share/pipewire/pipewire-pulse.conf.d/70-wolos-latency.conf");
    let has = |l: &str| t.lines().any(|x| x.trim() == l);
    assert!(has("pulse.min.quantum = 256/48000"), "floor");
    assert!(has("pulse.min.quantum = 64/48000") && has("{ application.process.binary = \"~wine.*\" }"), "games");
    assert!(has("pulse.min.quantum = 1024/48000") && has("{ cpu.vm.name = !null }"), "vm");
}
