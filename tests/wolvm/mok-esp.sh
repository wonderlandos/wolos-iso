#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# mok-esp.sh OUT.img SHIM_DIR SECOND_STAGE.efi: a FAT disk that boots like a WolOS install on a PC
# whose keys can't be changed: Fedora's signed shim (wolos-shim: shimx64.efi, mmx64.efi) as the
# removable-media loader and SECOND_STAGE as grubx64.efi. Under Secure Boot with Microsoft's keys,
# shim starts; a second stage signed by a key not in MokList (e.g. the ISO's systemd-boot) is
# refused and shim opens MokManager, the blue key screen. Journey: journeys/mok-screen.yaml.
# Needs mtools. Example (forge): SHIM_DIR=/usr/share/wolos/shim from the wolos-shim package.
set -eu
out=$1 shim=$2 second=$3
rm -f "$out"; truncate -s 64M "$out"
mformat -i "$out" -F -v WOLVM_MOK ::
mmd -i "$out" ::/EFI ::/EFI/BOOT
mcopy -i "$out" "$shim/shimx64.efi" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$out" "$shim/mmx64.efi" ::/EFI/BOOT/mmx64.efi
mcopy -i "$out" "$second" ::/EFI/BOOT/grubx64.efi
mdir -i "$out" ::/EFI/BOOT
