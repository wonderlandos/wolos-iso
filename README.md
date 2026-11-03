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

- Done in `wolos-base`: uutils-coreutils (as `coreutils`), sudo-rs (as `sudo`), bsdtar. The image lists no `gcc`, `texinfo`, `man-pages` or `base-devel` (tested in `checks/tests/image_gaming_audio.rs`); `base` does not pull them.
- Kept on purpose: glibc and gcc-libs (Steam, Proton, NVIDIA), bash (pacman hooks), GNU findutils, diffutils, tar (no uutils package at the snapshot; pacman/dkms scripts need them), `man-db` + `less` (small, used by `wol --help`).
- Not done: busybox/toybox. Arch packages depend on the GNU names, a second provider would add conflicts for no size gain.
- Recommendation: revisit when `uutils-findutils` and `uutils-diffutils` reach `extra`; add them to `wolos-base` `provides`/`conflicts` then.

## Tuning

wolos `files/usr/lib/sysctl.d/70-wolos-performance.conf`, `systemd/*.conf.d/70-wolos-limits.conf`, `systemd/*.conf.d/70-wolos-malloc.conf` (glibc thread cache: 64 chunks per bin instead of 7), `etc/security/limits.d`, and the PipeWire/WirePlumber drop-ins under wolos `files/usr/share` hold the game and audio defaults (values from CachyOS-Settings and Bazzite). PipeWire's minimum quantum is 64 frames so VMs keep working. No `PROTON_*` defaults are set globally: each is game-specific, so `wol games` sets them per title.
