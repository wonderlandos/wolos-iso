#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Secure Boot layout of the live ISO's EFI partition (ADR 0025), edited in place with mtools.
#   sb-layout.sh layout ISO REPO_DIR   shim first, systemd-boot second, one UKI (build.sh,
#                                      WOLOS_SB_LAYOUT=1)
#   sb-layout.sh sign ISO KEY CERT     sign systemd-boot and the UKI with the live-image key
#                                      (release/sb-key.sh), add its certificate and a README
#   sb-layout.sh check ISO             exit 0 only for a medium that boots with Secure Boot on:
#                                      a signed shim with an SBAT section, systemd-boot and the UKI
#                                      signed by the certificate it carries (wolos-build release),
#                                      and that certificate is the pinned live key
#                                      (release/wolos-live-2026.cer)
# WOLOS_SB_PIN_CERT=FILE (DER or PEM): dev/test only, the certificate `check` pins instead of
# release/<name>.cer (build.sh sets it when it signs with a WOLOS_SB_CERT of its own). `wolos-build
# release` (publish.sh) unsets it: a released medium always carries the WolOS live key.
# layout: EFI/BOOT/BOOTX64.EFI becomes the pinned shim (wolos-shim) with its key manager
# (mmx64.efi) next to it; systemd-boot moves to EFI/BOOT/grubx64.efi, the second stage that shim
# starts. The kernel, the initramfs and the two loader entries become one UKI with two profiles,
# Install WolOS and Try WolOS, in EFI/Linux/wolos.efi. Unsigned it boots with Secure Boot off (shim
# then starts anything). sign: systemd-boot and the UKI carry the live-image key's signature. With
# Secure Boot on, the first boot shows the one blue key screen; "Enroll key from disk" opens on the
# medium's root, where /ENROL_THIS_KEY_FOR_WOLOS.cer is the certificate to pick (also
# EFI/wolos/<name>.cer), and EFI/wolos/README.txt says what to press. Secure Boot off: never read.
set -euo pipefail
die() { echo "sb-layout.sh: $*" >&2; exit 1; }
SHIM_SHA=351e131d3c4a636704e9cae7d1296baf8a05beca9922cfebbfa48933c9bf1ad3 # wolos-pkgbuilds/wolos-shim
MM_SHA=ed4442fa88cccba3a9e6a32d21a8f4c56bf5aab184f1fb9aadde8d3a9d04dc27
UKIFY=$(command -v ukify || echo /usr/lib/systemd/ukify)
export MTOOLS_SKIP_CHECK=1
HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
ENROL=ENROL_THIS_KEY_FOR_WOLOS.cer

esp() {
  local start
  start=$(sfdisk -J "$1" | python3 -c '
import json, sys
p = json.load(sys.stdin)["partitiontable"]["partitions"]
print(next(x["start"] for x in p if x["type"].lower() in ("ef", "c12a7328-f81f-11d2-ba4b-00a0c93ec93b")))')
  echo "$1@@$((start * 512))"
}

layout() {
  local iso=$1 rd=$2 e t shim k i
  e=$(esp "$iso"); t=$(mktemp -d); trap 'rm -rf "$t"' RETURN
  shim=$(ls "$rd"/x86_64/wolos-shim-*.pkg.tar.zst 2>/dev/null | head -1)
  [ -n "$shim" ] || die "no wolos-shim package in $rd/x86_64"
  bsdtar -xf "$shim" -C "$t" usr/share/wolos/shim
  printf '%s  %s\n' "$SHIM_SHA" "$t/usr/share/wolos/shim/shimx64.efi" "$MM_SHA" "$t/usr/share/wolos/shim/mmx64.efi" |
    sha256sum -c --quiet || die "shim does not match its pin"
  mcopy -n -i "$e" ::/EFI/BOOT/BOOTx64.EFI "$t/systemd-boot.efi"
  mcopy -n -i "$e" ::/loader/entries/01-wolos.conf ::/loader/entries/02-wolos-try.conf "$t/"
  k=$(sed -n 's/^linux *//p' "$t/01-wolos.conf"); i=$(sed -n 's/^initrd *//p' "$t/01-wolos.conf")
  mcopy -n -i "$e" "::$k" "$t/vmlinuz"; mcopy -n -i "$e" "::$i" "$t/initrd"
  opt() { sed -n 's/^options *//p' "$t/$1"; }
  printf 'NAME="WolOS"\nPRETTY_NAME="WolOS"\nID=wolos\nID_LIKE=arch\n' > "$t/os-release"
  # Profile 0 (Install) takes the base command line; profile 1 (Try) has its own.
  "$UKIFY" build --profile=$'ID=install\nTITLE=Install WolOS' --output="$t/p0.efi" >/dev/null
  "$UKIFY" build --profile=$'ID=try\nTITLE=Try WolOS' --cmdline="$(opt 02-wolos-try.conf)" --output="$t/p1.efi" >/dev/null
  "$UKIFY" build --linux="$t/vmlinuz" --initrd="$t/initrd" --cmdline="$(opt 01-wolos.conf)" \
    --os-release="@$t/os-release" --join-profile="$t/p0.efi" --join-profile="$t/p1.efi" \
    --output="$t/wolos.efi" >/dev/null
  # The UKI holds kernel and initramfs: their loose copies and the entries leave the partition.
  mdel -i "$e" "::$k" "::$i" ::/loader/entries/01-wolos.conf ::/loader/entries/02-wolos-try.conf
  # No default entry name: profile 0 (Install) sorts first, as 01-wolos.conf did.
  mcopy -n -i "$e" ::/loader/loader.conf "$t/loader.conf"
  sed -i '/^default /d' "$t/loader.conf"
  mcopy -o -i "$e" "$t/loader.conf" ::/loader/loader.conf
  mmd -D s -i "$e" ::/EFI/Linux 2>/dev/null || :
  mcopy -o -i "$e" "$t/wolos.efi" ::/EFI/Linux/wolos.efi
  mcopy -o -i "$e" "$t/systemd-boot.efi" ::/EFI/BOOT/grubx64.efi
  mcopy -o -i "$e" "$t/usr/share/wolos/shim/shimx64.efi" ::/EFI/BOOT/BOOTX64.EFI
  mcopy -o -i "$e" "$t/usr/share/wolos/shim/mmx64.efi" ::/EFI/BOOT/mmx64.efi
  echo "sb-layout: shim -> systemd-boot -> EFI/Linux/wolos.efi (2 profiles) in $iso"
}

# unsign FILE: drop every signature already on FILE (an image signed again carries the new one only).
unsign() {
  while signed "$1"; do sbattach --remove "$1" >/dev/null 2>&1 || die "$1: cannot remove its old signature"; done
}
signed() { sbverify --list "$1" 2>/dev/null | grep -q '^signature [0-9]'; }

sign() {
  local iso=$1 key=$2 cert=$3 e t f name
  e=$(esp "$iso"); t=$(mktemp -d); trap 'rm -rf "$t"' RETURN
  name=$("$HERE/sb-key.sh" name)
  mcopy -n -i "$e" ::/EFI/Linux/wolos.efi "$t/wolos.efi" 2>/dev/null || die "no Secure Boot layout in $iso (sb-layout.sh layout)"
  mcopy -n -i "$e" ::/EFI/BOOT/grubx64.efi "$t/grubx64.efi"
  for f in wolos.efi grubx64.efi; do
    unsign "$t/$f"
    sbsign --key "$key" --cert "$cert" --output "$t/$f.signed" "$t/$f" >/dev/null 2>&1 || die "$f: signing failed"
    sbverify --cert "$cert" "$t/$f.signed" >/dev/null 2>&1 || die "$f: signature does not verify"
  done
  openssl x509 -in "$cert" -outform DER -out "$t/$name.cer"
  # For the person who meets the blue key screen first. The installer's words
  # (copy_installer.rs SECURE_CHOICE_SCREEN, KEY_SCREEN_LOOKS); DOS line ends for any editor.
  sed 's/$/\r/' > "$t/README.txt" <<TXT
WolOS and Secure Boot

Secure Boot off: nothing changes. WolOS starts straight away; ignore this file.

Secure Boot on: one blue screen appears before WolOS starts and asks you to
accept the key of this medium, once. It looks like an old computer screen:
blue, white letters, a short menu. The mouse does not work there: use the
arrow keys and Enter.

  1. "Verification failed": press Enter.
  2. "Press any key": press a key within 10 seconds.
  3. Choose "Enroll key from disk", then the volume that holds
     $ENROL (your USB stick, often the last one in the list;
     if you see EFI\Microsoft, press Esc and pick the next one), then
     $ENROL
  4. Choose "Continue", then "Yes".
  5. Choose "Reboot". WolOS starts, and the blue screen does not come back.

Missed the 10 seconds? Start the computer from this medium again: the blue
screen comes back. The same key is also in EFI\wolos\$name.cer.

This key starts the WolOS medium only. An installed WolOS uses its own key;
the installer says what to expect before its restart.
TXT
  mcopy -o -i "$e" "$t/wolos.efi.signed" ::/EFI/Linux/wolos.efi
  mcopy -o -i "$e" "$t/grubx64.efi.signed" ::/EFI/BOOT/grubx64.efi
  mmd -D s -i "$e" ::/EFI/wolos 2>/dev/null || :
  mcopy -o -i "$e" "$t/$name.cer" "::/$ENROL"
  mcopy -o -i "$e" "$t/$name.cer" "::/EFI/wolos/$name.cer"
  mcopy -o -i "$e" "$t/README.txt" ::/EFI/wolos/README.txt
  echo "sb-layout: signed systemd-boot and the UKI; /$ENROL, EFI/wolos/$name.cer and EFI/wolos/README.txt in $iso"
}

# sections FILE: the PE section names of FILE, one a line.
sections() {
  python3 -c '
import struct, sys
d = open(sys.argv[1], "rb").read()
o = struct.unpack_from("<I", d, 0x3c)[0]
if d[o:o + 4] != b"PE\0\0":
    sys.exit(1)
n, opt = struct.unpack_from("<H", d, o + 6)[0], struct.unpack_from("<H", d, o + 20)[0]
for i in range(n):
    s = o + 24 + opt + 40 * i
    print(d[s:s + 8].rstrip(b"\0").decode(errors="replace"))' "$1"
}

# fp FILE: the SHA-256 fingerprint of a certificate, DER or PEM; empty when it is neither.
fp() {
  openssl x509 -noout -fingerprint -sha256 -inform DER -in "$1" 2>/dev/null ||
    openssl x509 -noout -fingerprint -sha256 -inform PEM -in "$1" 2>/dev/null || :
}

# check ISO: what firmware with Secure Boot on needs from the medium; every failure is named.
check() {
  local iso=$1 e t f name bad=0 pin want
  name=$("$HERE/sb-key.sh" name)
  pin=${WOLOS_SB_PIN_CERT:-$HERE/$name.cer}
  want=$(fp "$pin"); [ -n "$want" ] || die "check: no pinned certificate at $pin"
  for f in mcopy sbverify openssl python3 sfdisk; do
    command -v "$f" >/dev/null || die "check needs $f (mtools, sbsigntools, openssl, python, util-linux)"
  done
  e=$(esp "$iso"); t=$(mktemp -d); trap 'rm -rf "$t"' RETURN
  mcopy -n -i "$e" ::/EFI/BOOT/BOOTX64.EFI "$t/boot.efi" 2>/dev/null || die "$iso: no EFI/BOOT/BOOTX64.EFI"
  signed "$t/boot.efi" || { echo "sb-layout check: EFI/BOOT/BOOTX64.EFI carries no signature (not the signed shim)" >&2; bad=1; }
  sections "$t/boot.efi" | grep -qx .sbat || { echo "sb-layout check: EFI/BOOT/BOOTX64.EFI has no .sbat section (not the signed shim)" >&2; bad=1; }
  if ! mcopy -n -i "$e" "::/$ENROL" "$t/key.cer" 2>/dev/null; then
    echo "sb-layout check: no /$ENROL (the certificate to enroll)" >&2; bad=1
  elif ! openssl x509 -inform DER -in "$t/key.cer" -out "$t/key.pem" 2>/dev/null; then
    echo "sb-layout check: /$ENROL is not a DER certificate" >&2; bad=1
  elif [ "$(fp "$t/key.cer")" != "$want" ]; then
    echo "sb-layout check: /$ENROL is not the WolOS live key (pinned: $pin)" >&2; bad=1; rm -f "$t/key.pem"
  fi
  if ! mcopy -n -i "$e" "::/EFI/wolos/$name.cer" "$t/k2.cer" 2>/dev/null; then
    echo "sb-layout check: no EFI/wolos/$name.cer" >&2; bad=1
  elif [ "$(fp "$t/k2.cer")" != "$want" ]; then
    echo "sb-layout check: EFI/wolos/$name.cer is not the WolOS live key (pinned: $pin)" >&2; bad=1
  fi
  for f in BOOT/grubx64.efi Linux/wolos.efi; do
    if ! mcopy -n -i "$e" "::/EFI/$f" "$t/x.efi" 2>/dev/null; then
      echo "sb-layout check: no EFI/$f" >&2; bad=1
    elif [ ! -s "$t/key.pem" ] || ! sbverify --cert "$t/key.pem" "$t/x.efi" >/dev/null 2>&1; then
      echo "sb-layout check: EFI/$f is not signed by the certificate in /$ENROL" >&2; bad=1
    fi
    rm -f "$t/x.efi"
  done
  [ "$bad" = 0 ] || return 1
  echo "sb-layout check: $iso boots with Secure Boot on (signed shim, systemd-boot and UKI signed by the pinned live key)"
}

case ${1:-} in
  layout) [ $# = 3 ] || die "usage: $0 layout ISO REPO_DIR"; layout "$2" "$3" ;;
  sign) [ $# = 4 ] || die "usage: $0 sign ISO KEY CERT"; sign "$2" "$3" "$4" ;;
  check) [ $# = 2 ] || die "usage: $0 check ISO"; check "$2" ;;
  *) die "usage: $0 layout ISO REPO_DIR | sign ISO KEY CERT | check ISO" ;;
esac
