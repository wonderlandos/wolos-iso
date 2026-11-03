// SPDX-License-Identifier: GPL-2.0-or-later
//! What this file checks (wolos-iso): The image ships the memory and latency sysctls and zram for linux-wolos.
//! linux-wolos (ADR 0016): the WolOS kernel fork at a pinned commit + Arch's config at the snapshot +
//! `linux-wolos/wolos.config` + extra patches from `$WOLOS_TREE/kernel/linux/patches` when present.
//! The kernel builds with makepkg; these tests pin what the recipe and its prepare step must do.
#![allow(dead_code, unused_imports)]
#[path = "support/siblings.rs"]
mod siblings;

use std::{fs, path::PathBuf, process::Command};
fn repo(p: &str) -> PathBuf {
    siblings::root().join(p)
}

fn read(p: &str) -> String {
    fs::read_to_string(repo(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Userspace half of the tuning: sysctl values, I/O schedulers and zram defaults ship in the image.
#[test]
fn image_ships_memory_and_latency_sysctls_and_zram() {
    let s = read("files/usr/lib/sysctl.d/70-wolos-performance.conf");
    for kv in [
        "vm.swappiness = 100",
        "vm.page-cluster = 0",
        "vm.vfs_cache_pressure = 50",
        "vm.dirty_bytes = 268435456",
        "vm.dirty_background_bytes = 67108864",
        "kernel.nmi_watchdog = 0",
        "kernel.sched_autogroup_enabled = 0",
        "kernel.watchdog_thresh = 30",
    ] {
        assert!(s.lines().any(|l| l == kv), "sysctl lacks {kv}");
    }
    let z = read("files/usr/lib/systemd/zram-generator.conf");
    assert!(z.contains("[zram0]") && z.contains("compression-algorithm = zstd"));
    assert!(
        read("profile/packages.x86_64")
            .lines()
            .any(|l| l == "zram-generator"),
        "profile/packages.x86_64 lacks zram-generator"
    );
    let t = read("files/usr/lib/tmpfiles.d/70-wolos-performance.conf");
    assert!(
        t.contains("/sys/kernel/mm/lru_gen/min_ttl_ms")
            && t.contains("transparent_hugepage/defrag")
    );
    let u = read("files/usr/lib/udev/rules.d/60-wolos-ioscheduler.rules");
    assert!(u.contains("none") && u.contains("bfq"));
    assert!(
        u.lines()
            .any(|l| l.contains("nvme") && l.contains("ATTR{queue/read_ahead_kb}=\"256\"")),
        "NVMe read_ahead_kb default"
    );
    // A slow USB stick has its own strict dirty budget: copying to it never stalls the NVMe.
    let w = read("files/usr/lib/udev/rules.d/70-wolos-usb-writeback.rules");
    for want in [
        "ENV{ID_BUS}==\"usb\"",
        "ATTR{removable}==\"1\"",
        "ATTR{bdi/strict_limit}=\"1\"",
        "ATTR{bdi/max_bytes}=\"33554432\"",
    ] {
        assert!(w.contains(want), "usb writeback rule lacks {want}");
    }
}
