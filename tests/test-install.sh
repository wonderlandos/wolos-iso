#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Unattended install test of the newest WolOS ISO in a throwaway VM (UEFI, no network), driven by
# wolvm (tests/wolvm/wolvm, tests/README-wolvm.md): this script only says what to make, start,
# press and check; wolvm owns the qemu command line, OVMF, swtpm and KVM-or-TCG. The ISO's kernel
# boots with wolos.autoinstall=/dev/vda (installs and powers off), then the installed disk boots on
# its own. A bystander disk (other.raw, random head) stays attached and must come out
# byte-identical. The boot is proven from the disk itself: systemd-bless-boot drops the entry's try
# counter (wolos-a+3.conf -> wolos-a.conf) only once boot-complete.target was reached, that is once
# the login window's supervisor ran. Results: lines "BOOT PASS|FAIL <what>" in $T/checks.txt;
# the exit status is 1 when any of them failed or none passed (tests/test/verdict.sh).
# Usage: tests/test-install.sh [ISO]          the newest ISO in $WOLOS_VM/out, or ISO
#        tests/test-install.sh --build [ISO]  ../build.sh first (the ISO lands in $WOLOS_VM/out)
#        tests/test-install.sh --release DIR  the artefacts `wolos-build release --iso DIR` publishes:
#          DIR's one wolos-*.iso, checked against the sha256 of its manifest (release.json, else the
#          <iso>.json build.sh wrote) and its <iso>.sha256 before the VM starts.
# KVM: with a usable /dev/kvm (the forge) wolvm runs the VM on KVM; without it on TCG (software
#      CPU, several times slower): the script says so and multiplies its timeouts by
#      WOLOS_TCG_SCALE (4).
# Env: WOLOS_VM (~/WolOS-vm), WOLOS_KEEP_DISK=1 keeps the installed disk as $T/disk.raw for
#      run-installed.sh (otherwise the scratch install.raw is deleted). WOLOS_KEEP_VM=1 keeps the
#      wolvm VM ($T/wolvm, name "install") for `wolvm --home $T/wolvm start install` and friends.
#      WOLOS_ENCRYPT=no|tpm|passphrase (default no): tpm adds a swtpm TPM2 that lives through install
#      and boot; passphrase puts the passphrase on a labelled key disk and types it at the unlock prompt.
#      WOLOS_SECBOOT=mok: OVMF's Secure Boot firmware with Microsoft's keys (wolvm --secureboot),
#      enforcement off for the install, on for the boot: the mok tier, whose first boot is
#      MokManager; its screens are driven by key presses (WOLOS_MOK_KEYS). Default (plain OVMF vars,
#      Setup Mode): the own-keys tier.
#      WOLOS_PLAN=auto (needs WOLOS_INSTALL_SCENARIO=<the install_scenario example binary of the wolos
#      repo>, WOLOS_ENCRYPT=no): instead of a plain erase the live system reads a plan the app would
#      have written (wolos.plan=WOLOSKEY:plan, autoinstall:50-56), made here for the blank 32G
#      /dev/vda (its fingerprint is that of a blank disk). Proves the plan was used: its account
#      (Alice Liddell) exists in the booted system.
#      WOLOS_BOOT_S (240): seconds the installed disk gets to boot (and enroll, and boot again).
#      Encrypted variants also ask the booted system whether any unit failed (a passphrase disk must
#      not run the chip's reseal, audit I6-1). tpm and mok boot the disk a SECOND time without
#      typing anything: the policy the first boot re-bound (PCR 7, +14 behind shim) must open the
#      disk on its own and no unlock prompt may reach the serial console (audit I6-4).
#      WOLOS_WOLVM: another wolvm (the checks run this script on a stub).
# Needs: wolvm's tools (qemu-system-x86_64, qemu-img, edk2-ovmf, bsdtar; swtpm for tpm,
#        virt-firmware for mok), blkid, sfdisk, mtools; python3 + Pillow for the screenshot check;
#        dosfstools for passphrase.
set -u
REPO=$(cd "$(dirname "$0")/.." && pwd)
VM=${WOLOS_VM:-$HOME/WolOS-vm}; T=$VM/test
ENC=${WOLOS_ENCRYPT:-no}; SB=${WOLOS_SECBOOT:-0}; PLAN=${WOLOS_PLAN:-no}
N=install
WOLVM=${WOLOS_WOLVM:-$REPO/tests/wolvm/wolvm}   # WOLOS_WOLVM: tests only (a stub wolvm)
w() { "$WOLVM" --home "$T/wolvm" "$@"; }
# wolvm picks KVM when this host can (read and write /dev/kvm), TCG otherwise: longer timeouts then.
if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
  scale=1
else
  scale=${WOLOS_TCG_SCALE:-4}
  echo "test-install: no usable /dev/kvm: running on TCG (software CPU), timeouts x$scale" >&2
fi
BOOT_S=${WOLOS_BOOT_S:-$((240 * scale))}; INSTALL_S=$((1800 * scale))
command -v qemu-system-x86_64 >/dev/null || { echo "$0: qemu-system-x86_64 missing (pacman -S qemu-desktop)" >&2; exit 1; }
command -v qemu-img >/dev/null || { echo "$0: qemu-img missing (pacman -S qemu-img)" >&2; exit 1; }
command -v mdir >/dev/null || { echo "$0: mtools missing (pacman -S mtools): the boot check reads the ESP with it" >&2; exit 1; }
case $ENC in no | tpm | passphrase) ;; *) echo "$0: WOLOS_ENCRYPT=$ENC: no, tpm or passphrase" >&2; exit 1 ;; esac
case $PLAN in
  no) ;;
  auto)
    [ "$ENC" = no ] || { echo "$0: WOLOS_PLAN=auto: the plan decides the protection: WOLOS_ENCRYPT=no" >&2; exit 1; }
    [ -x "${WOLOS_INSTALL_SCENARIO:-}" ] || { echo "$0: WOLOS_PLAN=auto needs WOLOS_INSTALL_SCENARIO=<install_scenario binary> (cargo build -p shell --example install_scenario)" >&2; exit 1; }
    command -v mkfs.vfat >/dev/null && command -v mcopy >/dev/null && command -v mmd >/dev/null || { echo "$0: WOLOS_PLAN=auto: dosfstools and mtools needed" >&2; exit 1; } ;;
  *) echo "$0: WOLOS_PLAN=$PLAN: auto or nothing" >&2; exit 1 ;;
esac
[ "$ENC" != tpm ] || command -v swtpm >/dev/null || { echo "$0: swtpm missing (pacman -S swtpm)" >&2; exit 1; }
[ "$SB" != mok ] || command -v virt-fw-vars >/dev/null || { echo "$0: virt-fw-vars missing (pacman -S virt-firmware)" >&2; exit 1; }
mkdir -p "$T"
# One VM at a time: another VM on the same disk and sockets starves the install (15 GB hosts).
pgrep -x qemu-system-x86 >/dev/null && { echo "test-install: another QEMU VM is running" >&2; exit 1; }
# release_iso DIR: the one ISO of a release directory, whose bytes match its manifest's sha256.
release_iso() {
  set -- "$1" "$1"/wolos-*.iso
  [ $# = 2 ] && [ -f "$2" ] || { echo "test-install: --release $1: needs exactly one wolos-*.iso there" >&2; return 1; }
  man=$1/release.json; [ -f "$man" ] || man=${2%.iso}.json
  [ -f "$man" ] || { echo "test-install: --release $1: no release.json or ${2##*/}: no manifest to check the ISO against" >&2; return 1; }
  want=$(tr -d ' \n' < "$man" | sed -n 's/.*"sha256":"\([0-9a-f]\{64\}\)".*/\1/p')
  have=$(sha256sum "$2" | cut -d' ' -f1)
  [ -n "$want" ] && [ "$want" = "$have" ] || { echo "test-install: ${2##*/} is $have, ${man##*/} says ${want:-nothing}: not the released image" >&2; return 1; }
  if [ -f "$2.sha256" ]; then
    (cd "$1" && sha256sum -c --status "${2##*/}.sha256") || { echo "test-install: ${2##*/}.sha256 does not match the ISO" >&2; return 1; }
  fi
  echo "$2"
}
case ${1:-} in
  --build) "$REPO/build.sh" || exit 1; shift ;;
  --release) ISO=$(release_iso "${2:?--release DIR}") || exit 1; set -- "$ISO" ;;
esac
ISO=${1:-$(ls -t "$VM"/out/wolos-*.iso 2>/dev/null | head -1)}
[ -f "$ISO" ] || { echo "test-install: no ISO in $VM/out (wolos-build iso, or ./build.sh)" >&2; exit 1; }
ISO=$(cd "$(dirname "$ISO")" && pwd)/$(basename "$ISO")
echo "test-install: $ISO ($(sha256sum "$ISO" | cut -d' ' -f1))"
# An archiso image: wolvm --direct boots its kernel with our command line (the ISO menu has no
# unattended entry).
bsdtar -tf "$ISO" wolos/boot/x86_64/vmlinuz-linux-wolos >/dev/null 2>&1 || { echo "test-install: no kernel in $ISO" >&2; exit 1; }
cd "$T" || exit 1
rm -rf plan; rm -f probe.json plan-scenario.log serial-install.log serial-boot.log serial-boot2.log checks.txt install.raw other.raw keys.raw shot*.png mok*.png unlock*.png facts-boot*.txt sb-state*.txt
w stop "$N" --force >/dev/null 2>&1; w rm "$N" >/dev/null 2>&1
truncate -s 17G other.raw
head -c 64M /dev/urandom | dd of=other.raw conv=notrunc status=none
# shellcheck disable=SC2034  # read inside the checks' eval strings
other_before=$(sha256sum other.raw | cut -d' ' -f1)
# Encryption words for the live kernel; the recovery key is printed so the run can be inspected.
words='' opts=''
case $ENC in
  tpm) words="wolos.encrypt=tpm wolos.recovery=console"; opts="--tpm" ;;
  passphrase)
    # The passphrase on its own small FAT disk, found by label from the live system.
    truncate -s 32M keys.raw && mkfs.vfat -n WOLOSKEY keys.raw >/dev/null || { echo "test-install: mkfs.vfat (dosfstools) failed" >&2; exit 1; }
    printf 'white rabbit' > passphrase.txt && mcopy -i keys.raw passphrase.txt ::passphrase && rm -f passphrase.txt
    words="wolos.encrypt=passphrase wolos.passphrase=WOLOSKEY:passphrase wolos.recovery=console"
    opts="--drive $T/keys.raw" ;;
esac
if [ "$PLAN" = auto ]; then
  # The plan the app would write for the blank /dev/vda (32 GiB): install_scenario walks the
  # installer's pages on that disk's probe (no table, no partitions), then the plan directory goes
  # on the WOLOSKEY disk, where the live system finds it by label.
  rm -rf plan && mkdir plan || exit 1
  python3 -c '
import json
b = 32 << 30
print(json.dumps({"path": "/dev/vda", "model": "", "bytes": b, "sector": 512, "table": "None", "id": "",
                  "first": 2048, "last": b // 512 - 34, "parts": []}))' > probe.json &&
    "$WOLOS_INSTALL_SCENARIO" erase probe.json plan > plan-scenario.log 2>&1 &&
    [ -s plan/plan.json ] || { echo "test-install: install_scenario made no plan (plan-scenario.log)" >&2; exit 1; }
  truncate -s 32M keys.raw && mkfs.vfat -n WOLOSKEY keys.raw >/dev/null && mmd -i keys.raw ::plan || { echo "test-install: mkfs.vfat/mmd failed" >&2; exit 1; }
  for f in plan/*; do mcopy -i keys.raw "$f" "::plan/${f##*/}" || { echo "test-install: mcopy $f failed" >&2; exit 1; }; done
  words="wolos.plan=WOLOSKEY:plan"; opts="--drive $T/keys.raw"
fi
# mok: Secure Boot firmware, Microsoft's keys in, enforcement off for the install (the unsigned
# live kernel must start) and on for the boot (what the person's PC does).
[ "$SB" != mok ] || opts="$opts --secureboot"
sb_enforce() { # on|off, VM stopped
  w set "$N" --secureboot "$1" >/dev/null 2>&1 && return 0
  # A wolvm without `set --secureboot`: flip SecureBootEnable in its qcow2 vars here.
  v=$T/wolvm/vms/$N/vars.qcow2 f=false; [ "$1" = on ] && f=true
  qemu-img convert -f qcow2 -O raw "$v" vars.raw &&
    virt-fw-vars --input vars.raw --output vars.sb.raw "--set-$f" SecureBootEnable >/dev/null 2>&1 &&
    qemu-img convert -f raw -O qcow2 vars.sb.raw "$v"
  r=$?; rm -f vars.raw vars.sb.raw; return $r
}
# vda = the VM's disk (32G qcow2), vdb = the bystander (raw, so its bytes are compared as they are).
# 2D virtio-vga (--gpu virtio): QMP screendump cannot read a GL scanout.
# shellcheck disable=SC2086  # opts are separate words
w new "$N" --iso "$ISO" --disk 32G --mem 4G --smp 4 --gpu virtio --direct --no-net \
  --drive "$T/other.raw" $opts --append "wolos.autoinstall=/dev/vda $words quiet" --force >/dev/null ||
  { echo "test-install: wolvm new failed" >&2; exit 1; }
[ "$SB" != mok ] || sb_enforce off || { echo "test-install: could not turn Secure Boot enforcement off" >&2; exit 1; }
running() { w status "$N" 2>/dev/null | grep -q '"running": true'; }
# 1. Install. The VM powers off when done (timeout 30 min, x4 on TCG).
w start "$N" >/dev/null || { echo "test-install: the VM did not start" >&2; exit 1; }
t=0; while running && [ "$t" -lt "$INSTALL_S" ]; do sleep 5; t=$((t + 5)); done
running && w stop "$N" --force >/dev/null 2>&1
w serial "$N" tail -n 1000000 > serial-install.log
# 2. Boot the installed disk alone (wolvm restarts the swtpm on its kept state).
w set "$N" --boot disk --no-iso >/dev/null
[ "$SB" != mok ] || sb_enforce on || { echo "test-install: could not turn Secure Boot enforcement on" >&2; exit 1; }
w start "$N" >/dev/null
shot() { w screenshot "$N" "$T/$1" >/dev/null 2>&1 || :; }
if [ "$SB" = mok ]; then
  # MokManager (shim's blue screen): any key, Enroll MOK, Continue, Yes, the password, Reboot. The
  # firmware shows it within seconds of the first boot; one shot per step keeps the evidence.
  sleep $((12 * scale)); shot mok1.png
  i=0
  for k in ${WOLOS_MOK_KEYS:-ret down ret down ret down ret b o o t ret ret}; do
    w key "$N" "$k" >/dev/null 2>&1; sleep $((2 * scale))
    i=$((i + 1)); [ $((i % 3)) != 0 ] || shot "mok$((i / 3 + 1)).png"
  done
fi
if [ "$ENC" = passphrase ]; then
  # The unlock prompt comes before any desktop; a few tries, as a person would, 15 s apart.
  for t in 1 2 3 4; do sleep $((15 * scale)); shot "unlock$t.png"; w type "$N" 'white rabbit\n' -e >/dev/null 2>&1 || :; done
fi
sleep "$BOOT_S"
shot shot.png
# guest_facts FILE: what the booted installed system says about itself, as "== section" blocks (the
# guest agent; no network needed). failed: units in the failed state. bind: wolos-tpm-bind (first
# boot: seals the disk key to PCR 7, +14 behind shim) and whether its install key is gone. reseal:
# wolos-tpm-reseal started by hand (a no-op unless the chip refused the disk; never a failure).
guest_facts() {
  w ssh "$N" 'echo "== state"; timeout 120 systemctl is-system-running --wait
echo "== failed"; systemctl --failed --no-legend --plain
echo "== bind"; systemctl show -p ConditionResult -p Result wolos-tpm-bind
test -e /var/lib/wolos/secureboot/rebind.key && echo rebind-key-left || echo rebind-key-gone
journalctl -b -u wolos-tpm-bind --no-pager -o cat
echo "== reseal"; systemctl start wolos-tpm-reseal; echo "start-rc=$?"
systemctl show -p Result wolos-tpm-reseal
journalctl -b -u wolos-tpm-reseal --no-pager -o cat
echo "== account"; getent passwd | grep -c "Alice Liddell"
echo "== end"' > "$1" 2>&1 || :
}
# sect FILE NAME: the lines of one "== NAME" block.
sect() { awk -v s="== $2" '/^== /{on=($0==s);next} on' "$1"; }
rm -f facts-boot1.txt
{ [ "$ENC" = no ] && [ "$PLAN" = no ]; } || guest_facts facts-boot1.txt
# mok: Secure Boot really on in the booted system (the guest agent; no network needed). Without
# it the mok tier would pass with shim starting anything.
rm -f sb-state.txt
[ "$SB" != mok ] || w ssh "$N" "mokutil --sb-state" > sb-state.txt 2>&1 || :
w stop "$N" --force >/dev/null 2>&1
w serial "$N" tail -n 1000000 > serial-boot.log
# 2b. tpm and mok: boot the disk AGAIN, typing nothing. The first start used the install-time policy
# (PCR 11), rebind then sealed the key to PCR 7 (+14): only a second start proves that policy opens
# the disk, and that MokManager has nothing left to ask. Waits for the guest agent, not a fixed time.
second=0; { [ "$ENC" = tpm ] || [ "$SB" = mok ]; } && second=1
if [ "$second" = 1 ]; then
  w start "$N" >/dev/null || { echo "test-install: the second start did not work" >&2; exit 1; }
  t=0; until w ssh "$N" true >/dev/null 2>&1 || [ "$t" -ge "$BOOT_S" ]; do sleep 5; t=$((t + 5)); done
  shot shot2.png
  guest_facts facts-boot2.txt
  [ "$SB" != mok ] || w ssh "$N" "mokutil --sb-state" > sb-state2.txt 2>&1 || :
  w stop "$N" --force >/dev/null 2>&1
  w serial "$N" tail -n 1000000 > serial-boot2.log
fi
# shellcheck disable=SC2034
other_after=$(sha256sum other.raw | cut -d' ' -f1)
# The installed disk as raw, read from outside below.
qemu-img convert -f qcow2 -O raw "$T/wolvm/vms/$N/disk.qcow2" install.raw 2>/dev/null || : > install.raw
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
  # No unit failed on an encrypted disk. The passphrase variant is the one that used to fail
  # (the chip's reseal ran on every encrypted start and found no chip), audit I6-1.
  if [ "$ENC" != no ]; then
    b "no failed unit after the first boot ($ENC)" "grep -q '^== end' facts-boot1.txt && [ -z \"\$(sect facts-boot1.txt failed)\" ]"
    b "wolos-tpm-reseal does not fail ($ENC)" "sect facts-boot1.txt reseal | grep -qx 'Result=success'"
  fi
  if [ "$PLAN" = auto ]; then
    b "the plan was used (its account exists)" "sect facts-boot1.txt account | grep -qx 1"
    b "plan install said done (wolos.plan=)" "grep -aq 'wolos-install: done' serial-install.log && ! grep -aq 'wolos-install: fail' serial-install.log"
  fi
  if [ "$ENC" = tpm ]; then
    b "first boot: wolos-tpm-bind ran and finished" "sect facts-boot1.txt bind | grep -qx 'Result=success' && sect facts-boot1.txt bind | grep -qx 'ConditionResult=yes' && sect facts-boot1.txt bind | grep -q 'Finished Bind the disk key'"
    b "first boot: the install key is gone after the bind" "sect facts-boot1.txt bind | grep -qx rebind-key-gone"
  fi
  if [ "$second" = 1 ]; then
    b "second boot: the guest answered without anyone typing" "grep -q '^== end' facts-boot2.txt"
    b "second boot: no unlock prompt on the console" "! grep -aiq 'enter passphrase\|enter recovery' serial-boot2.log"
    b "second boot: no failed unit" "grep -q '^== end' facts-boot2.txt && [ -z \"\$(sect facts-boot2.txt failed)\" ]"
    b "second boot: wolos-tpm-reseal ran clean" "sect facts-boot2.txt reseal | grep -qx 'start-rc=0' && sect facts-boot2.txt reseal | grep -qx 'Result=success'"
    b "second boot: the entry stayed blessed" "grep -q '/wolos-a.conf$' entries.txt"
    [ "$ENC" != tpm ] || b "second boot: the chip unlocked the disk (install key gone)" "sect facts-boot2.txt bind | grep -qx rebind-key-gone"
    [ "$SB" != mok ] || b "second boot: Secure Boot still on (no MokManager round)" "grep -q 'SecureBoot enabled' sb-state2.txt"
  fi
  [ "$SB" != mok ] || b "mok tier: shim first" "grep -q 'shimx64.efi' efi-wolos.txt && grep -q 'wolos-key.cer' efi-wolos.txt"
  [ "$SB" != mok ] || b "mok tier: Secure Boot enabled in the booted system" "grep -q 'SecureBoot enabled' sb-state.txt"
  # Informative: a UI screenshot has thousands of colours (firmware and text screens a handful).
  if [ -f shot.png ]; then
    echo "screenshot colours: $(python3 -c "from PIL import Image;print(len(Image.open('shot.png').convert('RGB').getcolors(1<<24) or []))" 2>/dev/null || echo unknown)"
  fi
} > checks.txt
grep '^BOOT ' checks.txt
rm -f other.raw keys.raw
if [ "${WOLOS_KEEP_DISK:-0}" = 1 ]; then mv install.raw "$T/disk.raw"; else rm -f install.raw; fi
[ "${WOLOS_KEEP_VM:-0}" = 1 ] || w rm "$N" >/dev/null 2>&1
echo "test-install: done ($T/checks.txt)"
sh "$REPO/tests/test/verdict.sh" BOOT checks.txt
