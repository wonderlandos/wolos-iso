#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# release/sb-release.sh ISO: the release workflow's Secure Boot step, run INSIDE a container by
# .github/workflows/release.yml (cwd /src = the workspace with wolos-iso/ and dist/, the signing key
# and certificate in /keys, the build box's checks.txt in /keys/checks.txt). The live Secure Boot key
# is mounted into that container, so the image is pinned by digest in the workflow and the packages
# come from the Arch archive date of this image (profile/snapshot), never from whatever the mirrors
# serve that minute (QA-A9-IMAGE-03). An image the build box already signed with the pinned live key
# keeps its tested bytes; otherwise the secrets must be that same key (a stray pair would give an
# image no PC boots). Then the image is booted in OVMF with Secure Boot on (tests/test-secureboot.sh);
# a runner whose KVM cannot run the firmware is a warning when the build box booted the same image
# with Secure Boot on (its checks file says so), not a verdict on the image.
set -euo pipefail
iso=${1:?usage: sb-release.sh ISO}
snap=$(tr -d '[:space:]' < wolos-iso/profile/snapshot)
[[ $snap =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || { echo "::error::wolos-iso/profile/snapshot is not a date"; exit 1; }
# shellcheck disable=SC2016 # $repo and $arch are pacman's, written literally
echo "Server = https://archive.archlinux.org/repos/${snap//-//}/"'$repo/os/$arch' > /etc/pacman.d/mirrorlist
pacman -Sy --noconfirm archlinux-keyring >/dev/null
pacman -Su --noconfirm --needed mtools sbsigntools openssl python util-linux qemu-base qemu-hw-display-virtio-vga edk2-ovmf virt-firmware >/dev/null
pin=wolos-iso/release/wolos-live-2026.cer
fp() { openssl x509 -noout -fingerprint -sha256 -inform DER -in "$1" 2>/dev/null || openssl x509 -noout -fingerprint -sha256 -inform PEM -in "$1"; }
if wolos-iso/release/sb-layout.sh check "$iso"; then
  echo "secure boot: already signed by the pinned live key; not signed again"
else
  [ "$(fp /keys/sb.crt)" = "$(fp "$pin")" ] || { echo "::error::WOLOS_SB_CERT is not the pinned live key ($pin)"; exit 1; }
  [ "$(openssl x509 -in /keys/sb.crt -noout -pubkey)" = "$(openssl pkey -in /keys/sb.key -pubout)" ] || { echo "::error::WOLOS_SB_KEY does not belong to WOLOS_SB_CERT"; exit 1; }
  wolos-iso/release/sb-layout.sh sign "$iso" /keys/sb.key /keys/sb.crt
fi
export WOLOS_SB_TEST=/src/dist/.sb-test
if ! wolos-iso/tests/test-secureboot.sh "$iso" wolos-iso/release/wolos-live-2026.crt; then
  echo "::group::qemu and serial logs"; cat /src/dist/.sb-test/qemu.log 2>/dev/null; tail -n 80 /src/dist/.sb-test/serial.log 2>/dev/null; echo "::endgroup::"
  if grep -qE "Secure Boot (still on|enabled in the booted system)" /keys/checks.txt; then
    echo "::warning::the runner could not boot the image with Secure Boot on, but the build box did (checks.txt); kept"
  else exit 1; fi
fi
rm -rf /src/dist/.sb-test
