#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Unattended install test of the newest WolOS ISO in a throwaway QEMU/KVM VM (UEFI, no network):
# the ISO's kernel boots with wolos.autoinstall=/dev/vda (installs and powers off), then the
# installed disk boots on its own. A bystander disk (other.raw, random head) stays attached and must
# come out byte-identical. The boot is proven from the disk itself: systemd-bless-boot drops the
# entry's try counter (wolos-a+3.conf -> wolos-a.conf) only once boot-complete.target was reached,
# that is once the login window's supervisor ran. Results: lines "BOOT PASS|FAIL <what>" in $T/checks.txt.
# Usage: tests/test-install.sh [ISO]   (--build: tests/build.sh first)
# Env: WOLOS_VM (~/WolOS-vm), WOLOS_KEEP_DISK=1 keeps the installed disk as $T/disk.raw for
#      run-installed.sh (otherwise the scratch install.raw is deleted).
#      WOLOS_ENCRYPT=no|tpm|passphrase (default no): tpm adds a swtpm TPM2 that lives through install
#      and boot; passphrase puts the passphrase on a labelled key disk and types it at the unlock prompt.
#      WOLOS_SECBOOT=mok: OVMF's Secure Boot firmware with Microsoft's keys (virt-fw-vars
#      --enroll-redhat), off for the install, on for the boot: the mok tier, whose first boot is
#      MokManager; its screens are driven by key presses (WOLOS_MOK_KEYS). Default (plain OVMF vars,
#      Setup Mode): the own-keys tier.
#      WOLOS_BOOT_S (240): seconds the installed disk gets to boot (and enroll, and boot again).
# Needs: qemu-system-x86_64, edk2-ovmf, bsdtar, blkid, sfdisk, mtools; python3 + Pillow for
#        screenshots; swtpm for tpm; virt-firmware for mok; dosfstools for passphrase.
set -u
REPO=$(cd "$(dirname "$0")/.." && pwd); H=$REPO/tests/test
VM=${WOLOS_VM:-$HOME/WolOS-vm}; T=$VM/test
OVMF=${WOLOS_OVMF:-/usr/share/edk2/x64}
ENC=${WOLOS_ENCRYPT:-no}; SB=${WOLOS_SECBOOT:-0}; BOOT_S=${WOLOS_BOOT_S:-240}
command -v qemu-system-x86_64 >/dev/null || { echo "$0: qemu-system-x86_64 missing (pacman -S qemu-desktop)" >&2; exit 1; }
[ -f "$OVMF/OVMF_CODE.4m.fd" ] || { echo "$0: no OVMF in $OVMF (pacman -S edk2-ovmf)" >&2; exit 1; }
command -v mdir >/dev/null || { echo "$0: mtools missing (pacman -S mtools): the boot check reads the ESP with it" >&2; exit 1; }
case $ENC in no | tpm | passphrase) ;; *) echo "$0: WOLOS_ENCRYPT=$ENC: no, tpm or passphrase" >&2; exit 1 ;; esac
[ "$ENC" != tpm ] || command -v swtpm >/dev/null || { echo "$0: swtpm missing (pacman -S swtpm)" >&2; exit 1; }
[ "$SB" != mok ] || command -v virt-fw-vars >/dev/null || { echo "$0: virt-fw-vars missing (pacman -S virt-firmware)" >&2; exit 1; }
mkdir -p "$T"
# One VM at a time: another VM on the same disk and sockets starves the install (15 GB hosts).
pgrep -x qemu-system-x86 >/dev/null && { echo "test-install: another QEMU VM is running" >&2; exit 1; }
if [ "${1:-}" = --build ]; then "$REPO/tests/build.sh" || exit 1; shift; fi
ISO=${1:-$(ls -t "$VM"/out/wolos-*.iso 2>/dev/null | head -1)}
[ -f "$ISO" ] || { echo "test-install: no ISO in $VM/out" >&2; exit 1; }
cd "$T" || exit 1
rm -f serial-install.log serial-boot.log checks.txt install.raw other.raw keys.raw shot*.png mok*.png kernel initrd tpm.sock
rm -rf tpm
# Direct kernel boot: the ISO's kernel and initramfs with our command line (the ISO menu has no
# unattended entry). archisosearchuuid comes from the ISO's label lookup instead: archisolabel.
bsdtar -xOf "$ISO" wolos/boot/x86_64/vmlinuz-linux-wolos > kernel &&
  bsdtar -xOf "$ISO" wolos/boot/x86_64/initramfs-linux-wolos.img > initrd || { echo "test-install: no kernel in $ISO" >&2; exit 1; }
label=$(blkid -o value -s LABEL "$ISO")
truncate -s 32G install.raw; truncate -s 17G other.raw
head -c 64M /dev/urandom | dd of=other.raw conv=notrunc status=none
# shellcheck disable=SC2034  # read inside the checks' eval strings
other_before=$(sha256sum other.raw | cut -d' ' -f1)
# Firmware. mok: Secure Boot firmware, Microsoft's keys in, enforcement off for the install (the
# unsigned live kernel must start) and on for the boot (what the person's PC does).
CODE=$OVMF/OVMF_CODE.4m.fd; MACHINE="-machine q35"
if [ "$SB" = mok ]; then
  CODE=$OVMF/OVMF_CODE.secboot.4m.fd; MACHINE="-machine q35,smm=on -global driver=cfi.pflash01,property=secure,value=on"
  virt-fw-vars --input "$OVMF/OVMF_VARS.4m.fd" --output vars.fd --enroll-redhat --set-false SecureBootEnable >/dev/null 2>&1 ||
    { echo "test-install: virt-fw-vars could not prepare the Secure Boot variables" >&2; exit 1; }
else
  cp "$OVMF/OVMF_VARS.4m.fd" vars.fd
fi
# Encryption words for the live kernel; the recovery key is printed so the run can be inspected.
words=
case $ENC in
  tpm)
    mkdir -p tpm
    swtpm socket --tpm2 --tpmstate dir="$T/tpm" --ctrl type=unixio,path="$T/tpm.sock" --daemon --terminate ||
      { echo "test-install: swtpm did not start" >&2; exit 1; }
    words="wolos.encrypt=tpm wolos.recovery=console" ;;
  passphrase)
    # The passphrase on its own small FAT disk, found by label from the live system.
    truncate -s 32M keys.raw && mkfs.vfat -n WOLOSKEY keys.raw >/dev/null || { echo "test-install: mkfs.vfat (dosfstools) failed" >&2; exit 1; }
    printf 'white rabbit' > passphrase.txt && mcopy -i keys.raw passphrase.txt ::passphrase && rm -f passphrase.txt
    words="wolos.encrypt=passphrase wolos.passphrase=WOLOSKEY:passphrase wolos.recovery=console" ;;
esac
# 2D virtio-vga: QMP screendump cannot read a GL scanout ("no surface"); Hyprland runs on Mesa.
GPU="-device virtio-vga -display none"
tpm_args() { [ "$ENC" = tpm ] && echo "-chardev socket,id=c,path=$T/tpm.sock -tpmdev emulator,id=t,chardev=c -device tpm-tis,tpmdev=t"; }
keys_args() { [ -f keys.raw ] && echo "-drive file=keys.raw,if=virtio,format=raw"; }
vm() { # extra qemu args...
  # shellcheck disable=SC2046,SC2086
  exec qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m 4G $MACHINE \
    -drive if=pflash,format=raw,readonly=on,file="$CODE" -drive if=pflash,format=raw,file=vars.fd \
    -drive file=install.raw,if=virtio,format=raw,discard=unmap -drive file=other.raw,if=virtio,format=raw $(keys_args) \
    -nic none $GPU -qmp unix:qmp.sock,server,nowait $(tpm_args) "$@"
}
# 1. Install: vda = install.raw, vdb = bystander. Powers off when done (timeout 30 min).
vm -cdrom "$ISO" -kernel kernel -initrd initrd \
  -append "archisobasedir=wolos archisolabel=$label wolos.autoinstall=/dev/vda $words console=ttyS0 quiet" \
  -serial file:serial-install.log &
qpid=$!; ( sleep 1800; kill $qpid 2>/dev/null ) & tpid=$!
wait $qpid; kill $tpid 2>/dev/null
# 2. Boot the installed disk alone. The swtpm is restarted on its kept state (it ends with the VM).
if [ "$ENC" = tpm ]; then
  swtpm socket --tpm2 --tpmstate dir="$T/tpm" --ctrl type=unixio,path="$T/tpm.sock" --daemon --terminate
fi
if [ "$SB" = mok ]; then
  virt-fw-vars --input vars.fd --output vars.sb.fd --set-true SecureBootEnable >/dev/null 2>&1 && mv vars.sb.fd vars.fd
fi
vm -serial file:serial-boot.log &
qpid=$!
shot() { python3 "$H/qmp.py" "$T" shot "$1" 2>/dev/null || :; }
if [ "$SB" = mok ]; then
  # MokManager (shim's blue screen): any key, Enroll MOK, Continue, Yes, the password, Reboot. The
  # firmware shows it within seconds of the first boot; one shot per step keeps the evidence.
  sleep 12; shot mok1.png
  i=0
  for k in ${WOLOS_MOK_KEYS:-ret down ret down ret down ret b o o t ret ret}; do
    python3 "$H/qmp.py" "$T" key "$k"; sleep 2
    i=$((i + 1)); [ $((i % 3)) != 0 ] || shot "mok$((i / 3 + 1)).png"
  done
fi
if [ "$ENC" = passphrase ]; then
  # The unlock prompt comes before any desktop; a few tries, as a person would, 15 s apart.
  for t in 1 2 3 4; do sleep 15; shot "unlock$t.png"; python3 "$H/qmp.py" "$T" keys 'white rabbit
' 2>/dev/null || :; done
fi
sleep "$BOOT_S"
shot shot.png
kill $qpid 2>/dev/null; wait $qpid 2>/dev/null
# shellcheck disable=SC2034
other_after=$(sha256sum other.raw | cut -d' ' -f1)
sfdisk -d install.raw > parts.txt 2>/dev/null
# The ESP from outside: its entries say whether the system came up (bless) and what was built.
esp_off=$(sfdisk -J install.raw 2>/dev/null | python3 -c '
import json, sys
p = json.load(sys.stdin)["partitiontable"]["partitions"]
print(next(x["start"] for x in p if x["type"].lower() == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b") * 512)' 2>/dev/null || echo 0)
MTOOLS_SKIP_CHECK=1 mdir -b -i "install.raw@@$esp_off" ::/loader/entries/ > entries.txt 2>/dev/null || :
MTOOLS_SKIP_CHECK=1 mdir -b -i "install.raw@@$esp_off" ::/EFI/wolos/ > efi-wolos.txt 2>/dev/null || :
b() { if eval "$2"; then echo "BOOT PASS $1"; else echo "BOOT FAIL $1"; fi; }
{ grep -a 'wolos-install:' serial-install.log
  cat parts.txt entries.txt
  b "installer said done" "grep -aq 'wolos-install: done' serial-install.log"
  b "bystander disk byte-identical" '[ "$other_before" = "$other_after" ]'
  b "ESP + root partitions" "grep -qi 'type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B' parts.txt && grep -qi 'type=4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709' parts.txt"
  b "boot image on the ESP" "grep -q 'wolos-a.efi' efi-wolos.txt"
  # The positive boot proof: the counted entry lost its counter, which only the booted system does.
  b "installed disk booted (entry blessed)" "grep -q '/wolos-a.conf$' entries.txt"
  # The firmware's own line names the entry it started: "Linux Boot Manager" (bootctl's) for the
  # own-keys and off tiers, "WolOS" (shim's) for mok.
  b "firmware started the disk" "grep -aq 'starting Boot.*\(Linux Boot Manager\|WolOS\)' serial-boot.log || grep -q '/wolos-a.conf$' entries.txt"
  case $ENC in
    tpm | passphrase)
      b "disk encrypted ($ENC)" "grep -aq 'wolos-install: encrypt 45' serial-install.log && grep -aq 'wolos-install: recovery ' serial-install.log" ;;
    no) b "disk not encrypted (default autoinstall)" "! grep -aq 'wolos-install: encrypt 45' serial-install.log" ;;
  esac
  [ "$SB" != mok ] || b "mok tier: shim first" "grep -q 'shimx64.efi' efi-wolos.txt && grep -q 'wolos-key.cer' efi-wolos.txt"
  # Informative: a UI screenshot has thousands of colours (firmware and text screens a handful).
  if [ -f shot.png ]; then
    echo "screenshot colours: $(python3 -c "from PIL import Image;print(len(Image.open('shot.png').convert('RGB').getcolors(1<<24) or []))" 2>/dev/null || echo unknown)"
  fi
} > checks.txt
grep '^BOOT ' checks.txt
rm -f other.raw kernel initrd keys.raw
if [ "${WOLOS_KEEP_DISK:-0}" = 1 ]; then mv install.raw "$T/disk.raw"; else rm -f install.raw; fi
echo "test-install: done ($T/checks.txt)"
