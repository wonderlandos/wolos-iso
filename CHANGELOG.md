# Changelog

The user-facing changes of a WolOS release are in the wolos repository's `CHANGELOG.md`; this file
lists what changed in the image, its checks and its tests. The four repositories are tagged together
(wolos ADR 0029).

## 0.12.1 (unreleased)

Image
- Package snapshot 2026-11-02: `linux-firmware` 20261019 (the 0.12.0 image had 20260916) and 31 other package updates; `profile/snapshot` and both `pacman.conf` files point at it.
- The builder image is pinned to a new digest in `.github/builder-image`.
- `linux-wolos` is 1:1.0+7.3.1-1 (stable Linux 7.3.1); the image checks that pin the kernel version (`checks/tests/image_profile.rs`) follow it.

Release
- `release.yml` keeps the build box's Secure Boot signature: an ISO already signed with the pinned live key is checked, not signed again, so the bytes the build box tested are the bytes released. Otherwise the signing secrets must be that pinned key (the certificate's fingerprint is compared with `release/wolos-live-2026.cer`, and the key must belong to the certificate); a stray pair stops the release.
- The runner's Secure Boot boot test prints the qemu and serial logs when it fails, and counts as a warning, not a failure, when the build box's `checks.txt` says it already booted the same image with Secure Boot on (a runner whose KVM cannot run the firmware says nothing about the image). The test container installs `qemu-hw-display-virtio-vga`.
- `checks.txt` is kept out of the uploaded files but available to that step; `out/` is created before the hand-mode release record is written.

Tests
- `tests/wolvm`: `stop` treats a guest that already powered itself off (a closed or resetting monitor) as going down, so the install-auto journey no longer reports a failure there; the wait for the process stays and the journey's stop step reports the real status.
- `test-modules.sh` resolves the module packages through a temporary copy of pacman's database with fresh repo lists, as `modules-install` does: on an installed system the database is on the read-only seal, so a plain `pacman -Sp` saw stale lists and reported a published module as missing.
- `test-modules.sh` checks Claude Code's managed settings and the WolOS skill only when the AI Assistants module (`wolos-module-ai-claude`) is installed; a plain install has neither, by design.
- New: `tests/guest-account.sh [USER]` makes the desktop account the guest tests run as (home, wheel/audio/video, lingering so the user manager and PipeWire start) in a VM from the unattended install, which creates no account (the Setup Assistant does on first boot).

## 0.12.0 (unreleased)

Image
- The live USB stick boots with Secure Boot on: the first-stage loader, the boot loader and the kernel image are signed with the WolOS live-image key, and the certificate to enrol sits on the startup partition as `ENROL_THIS_KEY_FOR_WOLOS.cer` (with `EFI/wolos/README.txt`). `build.sh` signs and checks the layout in one go wherever the key is.
- The release check refuses a medium signed by any other key: it compares against the WolOS certificate, not the one the medium carries.
- New packages on the image: the two X11 helpers the desktop uses for scaling and for allowing only this account's X11 apps, `gamescope` for games that need a fixed resolution, and `openssh` for the VM tests.
- Remote login stays off on the live medium; it starts only for a VM test that passes a login key.
- The medium README names the right volume on a PC with an internal disk.

Release
- `release.yml` no longer builds or VM-tests on the runner: it takes the ISO the build forge built and tested, and refuses one whose `checks.txt` does not say `BOOT PASS` or has a `FAIL`. It uploads the signed update feed (`stable.json` and its signature) with the ISO.
- The builder image is pinned by digest in `.github/builder-image`, the same digest as wolos-pkgbuilds.

Tests
- `tests/wolvm`: drive the whole system in a VM step by step: boot the ISO, go through the installer, start the installed disk, open apps, snapshots, Secure Boot on or off, extra drives, no network, and login over ssh (README: `tests/README-wolvm.md`).
- `test-install.sh` and `test-installer.sh` exit non-zero when a check fails. `test-install.sh --release DIR` checks the ISO against its recorded sha256 before any VM starts.
- The key-screen (MOK) variant fails when Secure Boot is off; the live-boot and unattended-install journeys check the booted system; the dual-boot run checks the dual-boot fixes it is meant to prove, including installing again over an earlier WolOS.
- New: `test-modules.sh` (optional modules), checks for the live Secure Boot layout, the live ssh drop-in and executable bits of the shipped scripts.
- `tests/build.sh` is gone; the image checks under `checks/` are rustfmt-formatted.
