# wolos-iso

The WolOS live + installer ISO: an archiso profile on Arch Linux packages pinned to one Arch Linux
Archive date (ADR 0016, 0017) plus the published `[wolos]` repo. **The ISO is built only from the
published repo** (+ the Arch snapshot), never from a source tree, so an image is reproducible from
(repo snapshot + this repo's commit). How to build: [BUILD.md](BUILD.md).

| Path | What |
|---|---|
| `profile/` | mkarchiso profile (releng-based): `snapshot`, `pacman.conf` (`[wolos]` first, then core/extra/multilib at the snapshot), `packages.x86_64` + `packages.desktop` (merged at build), systemd-boot entries (live, Install WolOS), live-only `airootfs` |
| `build.sh` | repo `snapshot` == `profile/snapshot` check -> staged profile (the published repo, checked against `release/wolos-repo.pub.asc`; the live root names its channel) -> `mkarchiso` -> ISO + `<iso>.sha256` + release manifest |
| `release/release-manifest.sh` | `release.json`: `{version, snapshot, wolos_repo, kernel, image, sha256, size}`; version and kernel from the repo database |
| `release/sign-release.sh` | `release.json.sig`: raw Ed25519 with the package key |
| `release/sb-layout.sh` | Secure Boot (ADR 0025): `layout` (shim, systemd-boot, one UKI), `sign` with the live-image key (+ `/ENROL_THIS_KEY_FOR_WOLOS.cer`, `EFI/wolos/README.txt`), `check` (what `wolos-build release` requires) |
| `release/sb-key.sh`, `release/wolos-live-2026.crt`/`.cer` | the live-image key's path where this host can sign (the forge; exit 3 elsewhere) and its certificate; `wolos-build iso` turns the Secure Boot layout on when the key is there (wolos-build `docs/signing-kms.md`) |
| `tests/` | QEMU/KVM + OVMF + virtio-gpu: `run.sh` (live), `test-install.sh` (unattended; `--release DIR` for release artefacts), `test-installer.sh` (end to end), `test-secureboot.sh`, `run-installed.sh`, `run-gpu.sh` (host iGPU) |
| `checks/` | contract tests of these files (Rust, std only): `cargo test --manifest-path checks/Cargo.toml` |

## Build and test (one path)

```sh
wolos-build iso                   # on the forge (or --ecs): ../wolos-iso/build.sh, ISO in --out DIR
tests/test-install.sh DIR/wolos-*.iso          # unattended install + boot, on the forge
tests/test-install.sh --release DIR            # what `wolos-build release --iso DIR` publishes
```

`build.sh` is the only build: `wolos-build iso` runs it on the forge or on the ECS cluster; by hand it
needs an Arch host with `archiso` (no container fallback). It refuses to start when
`$URL/snapshot` (written by `wolos-build publish`) differs from `profile/snapshot`, because the image
would then mix packages built against one Arch date with a base pinned to another. The live root's
`pacman.conf` names the repo and release tag the image came from (`[wolos-testing]` or `[wolos]` +
`Server = URL`); `install-run` (wolos `install-channel`) gives the installed system the same channel.
Next to the ISO it writes `<iso>.sha256` and the manifest `<iso>.json`; inside the ISO mkarchiso's
`wolos/x86_64/airootfs.sha512` is the hash the live medium check reads (build.sh refuses an ISO
without it).

VM tests run on the forge, which has `/dev/kvm`, QEMU, swtpm and OVMF (`/usr/share/edk2/x64`). On a
host without KVM, `tests/test-install.sh` says so, runs on TCG (software CPU) and multiplies its
timeouts by `WOLOS_TCG_SCALE` (4).

## Verify the download

Download the ISO, `wolos-<version>-x86_64.iso.sha256`, `release.json` and `release.json.sig` from
the release, then in that folder:

```sh
sha256sum -c wolos-*.iso.sha256                  # the ISO is the file the release names
grep -o '"sha256": *"[0-9a-f]*"' release.json    # the same hash, inside the signed manifest
for k in /usr/share/wolos/keys/wolos-*.pub; do
  openssl pkeyutl -verify -rawin -pubin -inkey "$k" -in release.json -sigfile release.json.sig && break
done
```

The loop checks that WolOS signed the manifest (Ed25519, the package key; the public keys are in
wolos `desktop/overlay/usr/share/wolos/keys/`, on an installed system `/usr/share/wolos/keys/`). It
prints `Signature Verified Successfully`. When the hash in `release.json` matches the ISO's, the
ISO is the one WolOS released. With `wolos-medium-verify.service` (plan W5, profile) a booted medium
checks itself too: the live system hashes its squashfs against `airootfs.sha512` in the background,
and the installer refuses to write a disk from a corrupt stick.

## Release flow

1. Packages land in `[wolos-testing]` (wolos-pkgbuilds `build.yml`, then the publish button in wolos: `what=packages`).
2. Release `vX.Y.Z` from the publish button in wolos (`what=iso -f run=vX.Y.Z`; the signing keys live there): this repo's reusable `release.yml` builds the ISO on the ECS cluster
   from `[wolos-testing]` (`wolos-build iso --ecs`), boots it in a VM (`tests/test-install.sh`;
   with `WOLOS_SB_KEY`/`WOLOS_SB_CERT` also Secure Boot), signs `release.json`, and publishes the ISO,
   `.sha256`, `release.json` and `.sig` as the latest release of `wonderlandos/wolos-packages`.
3. The tested packages are promoted to `[wolos]` (publish button, `what=promote`).
4. Users: `wolos-update-check.timer` fetches `releases/latest/download/release.json` (+ `.sig`);
   Software Update or `wolos-update-slot` runs `wol update apply` into the other slot.

Move a release to a newer snapshot: change `profile/snapshot`, the dates in both `pacman.conf` files,
and in wolos-pkgbuilds `snapshot`, `pacman.conf` and `wolos-mirrorlist/` (file + pkgver).

Packages: `[wolos]` is served by the WolOS CDN (CloudFront + S3 `wolos-repo`, wolos-build `infra/cdn.tf`),
with the GitHub release assets as fallback. The CachyOS x86-64-v3 repos (`[cachyos-v3]`,
`[cachyos-core-v3]`, `[cachyos-extra-v3]`, off until `wol mirror gate` turns them on for a v3 CPU) are
not republished: the CDN answers `/cachy/<repo>/<file>` with a 302 to a ranked CachyOS mirror
(wolos ADR 0030). `Architecture = x86_64 x86_64_v3` because those packages carry arch `x86_64_v3`.
Boot chain and installer: wolos `docs/modules/boot.md`.

## Userland and GNU

Decided 2026-11-04 (order: switch to uutils what is already stable, as Ubuntu did). The gate is
`wolos-base/userland-matrix.sh` in wolos-pkgbuilds: 170 cases, one per way WolOS code (`ours-`) and the Arch
tools on every system (`third-`: makepkg, mkinitcpio, dkms, update-ca-trust, devtools) call the tool,
each with the answer GNU printed. One `ours-`/`third-` difference means the tool is not switched, unless
the script changes. `WOLOS_USERLAND_MATRIX=1 cargo test --test recipe_userland_uutils` runs it against
the tools in `$PATH` (the image, a VM, or a directory of candidates first in `$PATH`).

| Tool | In wolos-base | Evidence (2026-11-04) |
|---|---|---|
| coreutils | uutils-coreutils 0.12.0 as `coreutils` (exactly GNU 9.12's 102 names are linked) | In `extra`. GNU test suite 666 of 720 pass. Ubuntu 26.10 ships the same 0.12.0 with `cp`/`mv`/`rm` too; 25.10 and 26.04 kept those three GNU for open TOCTOU bugs (8 on 2026-04-22). Matrix: 52 of 52 `ours-` cases equal; `timeout -s INT -k N` exits 137 where GNU exits 124 (nothing here reads it) |
| sudo | sudo-rs as `sudo` | Ubuntu's default since 25.10 |
| findutils | stays GNU | Not in Arch `extra` (AUR only), so a swap means building it ourselves from crates.io 0.11.0, released 2026-11-02. GNU test suite 437 of 495 (bfs suite 280 of 317). Matrix: every `ours-`/`third-` case equal; `-ignore_readdir_race` and `xargs -t` differ (unused). Ubuntu keeps `rust-findutils` in universe, not default. Size: `find` alone is 3.9 MB, GNU findutils 1.9 MiB installed, so it would add to the image |
| diffutils | stays GNU | Not in `extra`. 0.6.0: GNU suite 19 of 33. Fails 8 `third-` cases: no `-r -q -w -p -N`, `--label`, `--color`, `--side-by-side`, `--version` (devtools `checkpkg`/`diffpkg`, dkms, libtool use them). `cmp -s` (boot-setup) passes. `mkinitcpio` depends on it anyway |
| procps-ng | stays GNU | No release (tag 0.0.1), no package anywhere. Built from git: `ps -ax -o pid= -o ppid=` (wol-login `stop_tree`) prints nothing, `ps -o comm= -p` is wrong, no `uptime -p`, no `top -b`. `pgrep`/`pkill` pass |
| util-linux | stays GNU | uutils/util-linux 0.0.1 (AUR), no release; `mount`, `lsblk`, `blkid`, `sfdisk` are the installer's and the updater's |
| tar | stays GNU | pacman/dkms scripts use GNU options; bsdtar is shipped and preferred by makepkg and mkinitcpio |

- Kept on purpose: glibc and gcc-libs (Steam, Proton, NVIDIA), bash (pacman hooks), `man-db` + `less` (small, used by `wol --help`). The image lists no `gcc`, `texinfo`, `man-pages` or `base-devel` (tested in `checks/tests/image_gaming_audio.rs`, which also keeps the GNU tools above from being listed one by one or swapped without the matrix).
- Not done: busybox/toybox. Arch packages depend on the GNU names, a second provider would add conflicts for no size gain.
- Revisit when `uutils-findutils` reaches `extra` (then run the matrix, add it to `wolos-base` `provides`/`conflicts` as for coreutils) or when uutils-diffutils passes the `third-` cases. Sources: https://documentation.ubuntu.com/release-notes/26.04/summary-for-lts-users/ (`rust-coreutils`, `sudo-rs`), https://discourse.ubuntu.com/t/an-update-on-rust-coreutils/80773 (cp/mv/rm), https://discourse.ubuntu.com/t/carefully-but-purposefully-oxidising-ubuntu/56995 (findutils/diffutils as opt-in experiments), https://github.com/uutils/findutils-tracking and https://github.com/uutils/diffutils-tracking (suite results), Launchpad `rust-coreutils`, `rust-findutils`, `rust-diffutils` publishing history.

## Hardware support

Added 2026-11-04 (order: "use advanced drivers of Intel etc."). What a 2020-and-newer Intel or AMD laptop needs, what the image ships for it, and the `wol doctor hardware` row that reads it (wolos `docs/modules/hardware.md` has the long table and the reasons for each absence). `checks/tests/image_modern_hardware.rs` keeps the list, the absences and the doctor rows in step.

| Need | On the image (`profile/packages.*`) | Size added |
|---|---|---|
| Video decode (VA-API) | `mesa` (AMD), `intel-media-driver` (Intel Broadwell+), `libva-utils` | already shipped |
| Vulkan | `vulkan-intel` (anv, hasvk), `vulkan-radeon` (RADV), loader, tools, `lib32-` pair | already shipped |
| Wi-Fi, Bluetooth, GPU, audio firmware | `linux-firmware` (meta: every split part), `intel-ucode`, `amd-ucode`, `sof-firmware`, `wireless-regdb` | already shipped |
| Fingerprint reader | `fprintd` (+ `libfprint`) | 3.0 MiB installed |
| Thunderbolt, USB4 | `bolt` | 0.4 MiB |
| Cooling (Intel) | `thermald`, enabled by `wolos-system`'s preset | 0.7 MiB |
| Power modes | `power-profiles-daemon` (the only power daemon) | already shipped |

Left off (each with its reason in the wolos doc): the X11 DDX drivers (`xf86-video-*`, Wayland only), `intel-compute-runtime` and `intel-graphics-compiler` (393 MiB, on demand), `linux-firmware-qcom` (210 MiB, not x86), `tlp` and `auto-cpufreq` (a second power daemon), NVIDIA (installed online by the hardware catalogue).

## x86-64-v3

Evaluated 2026-11-04 for our own packages (`wolos-*`, `hyprland-wolos`, `rsh`); numbers, decision and the installer guard are in wolos `docs/modules/x86-64-v3.md`. The forge builds the standard `x86-64` flavour; `wolos-build pkg --march v3` builds the same recipes as `x86_64_v3` packages into a separate output (`out-v3`) for a `[wolos-v3]` repo that `wol mirror gate` would turn on with the CachyOS ones on AVX2 CPUs only.

## Tuning

wolos `files/usr/lib/sysctl.d/70-wolos-performance.conf`, `systemd/*.conf.d/70-wolos-limits.conf`, `systemd/*.conf.d/70-wolos-malloc.conf` (glibc thread cache: 64 chunks per bin instead of 7), `etc/security/limits.d`, and the PipeWire/WirePlumber drop-ins under wolos `files/usr/share` hold the game and audio defaults (values from CachyOS-Settings and Bazzite). PipeWire's minimum quantum is 64 frames so VMs keep working. No `PROTON_*` defaults are set globally: each is game-specific, so `wol games` sets them per title.
