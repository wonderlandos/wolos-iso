#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Installer end to end (ADR 0023): real disk images, the real install in a VM, each disk booted
# afterwards, what the person kept checked from outside. `install_scenario` walks the same steps
# as the app. Boot counting proves the boot: the entry is blessed only when the system came up.
# Usage: tests/test-installer.sh remaster BASE.iso BIN_DIR | tests/test-installer.sh run SCENARIO...
#        (BIN_DIR: wol, wol-installer, wol-shell, install_scenario; wol-xguard when it is there)
#        tests/test-installer.sh layouts BIN_DIR NAME...   (real partitioning on loop devices, no VM;
#          exit 1 on any E2E FAIL or when no layout passed, e.g. every name skipped)
# Scenarios: erase alongside-windows alongside-linux manual manual-ext4 password encrypted-alongside tpm medium
#            bitlocker-windows reinstall-windows replace offline-modules
# Exit status of `run`: 1 when any E2E FAIL line came out or a scenario left no E2E PASS line
# (tests/test/verdict.sh), else 0.
# VMs: wolvm (tests/wolvm/wolvm, tests/README-wolvm.md), one per scenario (e2e-<scenario> in
#      $WOLOS_E2E/wolvm): the scenario's raw disk goes in (--from-image) and comes back out raw for
#      the checks; KVM when /dev/kvm is usable, TCG otherwise.
# Env: WOLOS_E2E (work dir), WOLOS_E2E_INSTALL_S, WOLOS_E2E_BOOT_S (longer without /dev/kvm),
#      WOLOS_E2E_MODULES_S; WOLOS_E2E_REPO (+ WOLOS_E2E_REPO_KEY): offline-modules installs from
#      this local repo, signed with that key (a build not yet published).
# Needs: wolvm's tools (qemu, qemu-img, edk2-ovmf, bsdtar, swtpm), sudo, squashfs-tools, xorriso.
set -u
REPO=$(cd "$(dirname "$0")/.." && pwd); H=$REPO/tests/test
# The WolOS source the installer comes from: $WOLOS_SRC, else a wolos checkout next to this repo.
SRC=${WOLOS_SRC:-$(dirname "$REPO")/wolos}
W=${WOLOS_E2E:-$HOME/wolos-e2e}
ISO=$W/wolos-e2e.iso
mkdir -p "$W"

remaster() { # base.iso bin_dir: this tree's installer on the base image
  local base=$1 bin=$2 r=$W/root
  sudo rm -rf "$r"; xorriso -osirrox on -indev "$base" -extract /wolos/x86_64/airootfs.sfs "$W/base.sfs" 2>/dev/null
  sudo unsquashfs -q -f -d "$r" "$W/base.sfs" >/dev/null || exit 1
  sudo install -m 755 "$bin/wol" "$bin/wol-installer" "$bin/wol-shell" "$bin/install_scenario" "$r/usr/bin/"
  # The malware guard too when BIN_DIR has it (cargo build --release -p xguard --features yara).
  [ ! -x "$bin/wol-xguard" ] || sudo install -m 755 "$bin/wol-xguard" "$r/usr/bin/" || exit 1
  # This tree's desktop/overlay and files/ as wolos-system packages them (overlay first, files/
  # over it, usr/local left out), so hook and config changes reach the image without a package
  # build. Directories already in the image keep their owner and mode.
  (cd "$SRC/desktop/overlay" && tar --exclude ./usr/local -cf - .) |
    sudo tar -C "$r" --no-same-owner --no-overwrite-dir -xf - || exit 1
  sudo cp -r "$SRC"/files/. "$r/"
  sudo chmod 755 "$r"/usr/lib/wolos/*
  local f
  for f in "$SRC"/desktop/overlay/usr/bin/*; do sudo chmod 755 "$r/usr/bin/${f##*/}"; done
  sudo chmod 440 "$r/etc/sudoers" "$r/etc/sudoers.d/wolos"
  sudo cp -r "$REPO"/profile/airootfs/etc/systemd/system/. "$r/etc/systemd/system/"
  sudo cp "$H/installer/wolos-e2e" "$r/usr/lib/wolos/wolos-e2e"
  sudo cp "$H/installer/wolos-e2e.service" "$r/etc/systemd/system/"
  sudo ln -sf ../wolos-e2e.service "$r/etc/systemd/system/multi-user.target.wants/wolos-e2e.service"
  sudo sed -i '$a m live seat' "$r/etc/sysusers.d/wolos-live.conf"
  echo 'live ALL=(root) NOPASSWD: /usr/lib/wolos/install-run, /usr/lib/wolos/install-check, /usr/bin/systemctl reboot, /usr/bin/systemctl poweroff' | sudo tee "$r/etc/sudoers.d/wolos-live" >/dev/null
  # An image older than the package list: the newer tools, without moving what it already has.
  local db=$r/var/lib/pacman
  [ -d "$db/local" ] || db=$r/usr/lib/pacman
  sudo cp -r /var/lib/pacman/sync "$db/"
  sudo pacman -Sdd --noconfirm --needed --root "$r" --dbpath "$db" --hookdir "$W" --noscriptlet --overwrite '*' \
    xfsprogs ntfsprogs liburcu systemd-ukify python python-pefile sbctl mokutil >/dev/null || exit 1
  sudo rm -rf "$db/sync"
  [ -x "$r/usr/bin/ntfsresize" ] && [ -x "$r/usr/bin/mkfs.xfs" ] && [ -x "$r/usr/bin/ukify" ] || exit 1
  sudo rm -f "$W/new.sfs"
  sudo mksquashfs "$r" "$W/new.sfs" -comp zstd -Xcompression-level 3 -quiet -noappend || exit 1
  sudo chown "$(id -u)" "$W/new.sfs"
  rm -f "$ISO"
  xorriso -indev "$base" -outdev "$ISO" -boot_image any replay \
    -map "$W/new.sfs" /wolos/x86_64/airootfs.sfs -end 2>/dev/null
  ls -la "$ISO"
}

# Runs: `with_loop FILE cmd...` with $LOOP set and the partitions showing.
with_loop() {
  local backing=$1; shift
  LOOP=$(sudo losetup -Pf --show "$backing") || exit 1
  sudo udevadm settle
  "$@"; local rc=$?
  sudo losetup -d "$LOOP"
  return $rc
}

part_hash() { # disk.raw number -> hash of the partition's bytes
  local line start size
  line=$(sfdisk -d "$1" | grep -E "^$1$2 ")
  start=$(sed -E 's/.*start= *([0-9]+).*/\1/' <<<"$line"); size=$(sed -E 's/.*size= *([0-9]+).*/\1/' <<<"$line")
  dd if="$1" bs=1M iflag=skip_bytes,count_bytes skip=$((start * 512)) count=$((size * 512)) status=none | sha256sum | cut -d' ' -f1
}

mk_windows_inner() {
  sudo mkfs.vfat -n SYSTEM "${LOOP}p1" >/dev/null
  # The boot sector as Windows writes it: on a loop partition mkntfs can not read the start and the
  # geometry itself (it warns "Windows will not be able to boot from this device" and writes 0),
  # so they are given; the installer never formats NTFS, it only shrinks it (ntfsresize).
  sudo mkntfs -Q -L Windows -p "$(cat "/sys/class/block/${LOOP#/dev/}p3/start")" -H 255 -S 63 "${LOOP}p3" >/dev/null
  local m=$D.mnt; mkdir -p "$m"
  sudo mount "${LOOP}p1" "$m"; sudo mkdir -p "$m/EFI/Microsoft/Boot"; echo fake | sudo tee "$m/EFI/Microsoft/Boot/bootmgfw.efi" >/dev/null; sudo umount "$m"
  sudo mount -t ntfs3 "${LOOP}p3" "$m" 2>/dev/null || sudo ntfs-3g "${LOOP}p3" "$m"
  sudo mkdir -p "$m/Windows/System32" "$m/Users/Alice"
  sudo dd if=/dev/urandom of="$m/Users/Alice/keep.bin" bs=1M count=256 status=none
  sudo sha256sum "$m/Users/Alice/keep.bin" | cut -d' ' -f1 > "$D.keep"
  sudo umount "$m"
}

mk_linux_inner() {
  sudo mkfs.vfat -F32 -n ESP "${LOOP}p1" >/dev/null
  local e=$D.mnt; mkdir -p "$e"
  sudo mount "${LOOP}p1" "$e"; sudo mkdir -p "$e/EFI/fedora"
  sudo dd if=/dev/urandom of="$e/EFI/fedora/grubx64.efi" bs=1M count=4 status=none
  sudo sha256sum "$e/EFI/fedora/grubx64.efi" | cut -d' ' -f1 > "$D.esp"
  sudo umount "$e"
  sudo mkfs.ext4 -q -L linuxroot "${LOOP}p2"
  sudo mkfs.ext4 -q -L linuxhome "${LOOP}p3"
  sudo mkswap -q "${LOOP}p4"
  local m=$D.mnt; mkdir -p "$m"
  sudo mount "${LOOP}p2" "$m"
  sudo mkdir -p "$m/etc"; printf 'NAME="Fedora Linux"\nPRETTY_NAME="Fedora Linux 41"\n' | sudo tee "$m/etc/os-release" >/dev/null
  sudo umount "$m"
  sudo mount "${LOOP}p3" "$m"
  sudo mkdir -p "$m/alice"; sudo dd if=/dev/urandom of="$m/alice/keep.bin" bs=1M count=64 status=none
  sudo sha256sum "$m/alice/keep.bin" | cut -d' ' -f1 > "$D.keep"
  sudo umount "$m"
}

# bitlocker_header FILE OFFSET: a BitLocker (Windows 7 and later) volume header at byte OFFSET of
# FILE: the jump and "-FVE-FS-" where NTFS has its jump and "NTFS    ", the first metadata offset
# (64 KiB, the u64 at byte 176) and the metadata block's own "-FVE-FS-" there. blkid needs all
# three: "-FVE-FS-" alone is no file system at all (checked with util-linux 2.42).
bitlocker_header() {
  printf '\353\130\220-FVE-FS-' | dd of="$1" bs=1 seek="$2" conv=notrunc status=none
  printf '\000\000\001\000\000\000\000\000' | dd of="$1" bs=1 seek=$(($2 + 176)) conv=notrunc status=none
  printf -- '-FVE-FS-' | dd of="$1" bs=1 seek=$(($2 + 65536)) conv=notrunc status=none
}

mk_disk() { # kind path
  D=$2; rm -f "$D"; truncate -s 64G "$D"
  case $1 in
    blank) ;;
    windows)
      sfdisk -q "$D" <<'E'
label: gpt
size=100MiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B
size=16MiB, type=E3C9E316-0B5C-4DB8-817D-F92DF00215AE
type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7
E
      with_loop "$D" mk_windows_inner ;;
    bitlocker)
      # Windows whose system partition BitLocker protects (blkid then says TYPE=BitLocker, as on a
      # real PC). Install Alongside must refuse to shrink it and touch nothing.
      mk_disk windows "$D"
      local p3; p3=$(sfdisk -d "$D" | grep -E "^${D}3 " | sed -E 's/.*start= *([0-9]+).*/\1/')
      bitlocker_header "$D" $((p3 * 512)) ;;
    linux)
      sfdisk -q "$D" <<'E'
label: gpt
size=1GiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B
size=24GiB, type=4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709, name=linuxroot
size=30GiB, type=933AC7E1-2EB4-4F13-B844-0E14E2AEF915, name=linuxhome
type=0657FD6D-A4AB-43C4-84E5-0933C84B4F4F
E
      with_loop "$D" mk_linux_inner ;;
  esac
}

w() { "$REPO/tests/wolvm/wolvm" --home "$W/wolvm" "$@"; }
running() { w status "$1" 2>/dev/null | grep -q '"running": true'; }
wait_off() { # vm seconds: until the VM powered itself off, else stop it
  local t=0
  while running "$1" && [ "$t" -lt "$2" ]; do sleep 5; t=$((t + 5)); done
  running "$1" && w stop "$1" --force >/dev/null 2>&1
}
to_raw() { # vm dir: the VM's disk back as $dir/disk.raw (the checks below read it with losetup)
  # Right after `wolvm stop --force` a loaded host may still hold the qcow2's write lock ("Failed
  # to get shared write lock"): wait for it, a bounded while, then say so as a failed check.
  local i err=$2/to_raw.err
  for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
    qemu-img convert -f qcow2 -O raw "$W/wolvm/vms/$1/disk.qcow2" "$2/disk.raw" 2> "$err" && { rm -f "$err"; return 0; }
    grep -qi 'lock' "$err" || break
    sleep 5
  done
  echo "E2E FAIL ${S:-$1}: the disk did not come back from the VM after try $i (qemu-img: $(tr '\n' ' ' < "$err"))"
  return 1
}

install_in_vm() { # dir scenario-name plan
  local d=$1 n=e2e-$2 s=$3 start=$SECONDS tpm='' net=--no-net
  # tpm: a TPM2 that lives through install and boot (wolvm keeps its state), and a network card:
  # without one the first boot falls back to the recovery prompt (prerelease run 2026-11-03, B6;
  # the product side is tracked apart). The others stay offline.
  [ "$2" != tpm ] || { tpm=--tpm; net=; }
  # The disk keeps its size (a grown disk would move the GPT backup header the installer reads).
  # shellcheck disable=SC2086  # net is one word or none
  w new "$n" --from-image "$d/disk.raw" --disk "$(stat -c %s "$d/disk.raw")" --iso "$ISO" --direct $net \
    --mem 6G --smp 4 --gpu virtio $tpm --append "wolos.e2e=$s wolos.try quiet audit=0" --force >/dev/null || return 1
  w start "$n" >/dev/null || return 1
  wait_off "$n" "${WOLOS_E2E_INSTALL_S:-1500}"
  w serial "$n" tail -n 1000000 > "$d/install.log"
  to_raw "$n" "$d"
  echo "$2: install took $((SECONDS - start)) s" | tee "$d/took"
}

boot_in_vm() { # dir scenario: up, a screenshot, a clean stop
  local d=$1 s=$2 n=e2e-$2
  w set "$n" --boot disk --no-iso >/dev/null
  w start "$n" >/dev/null
  if [ "$s" = password ] || [ "$s" = encrypted-alongside ]; then
    for t in 1 2 3 4; do sleep 15; w screenshot "$n" "$d/unlock$t.png" >/dev/null 2>&1; w type "$n" 'white rabbit\n' -e >/dev/null 2>&1; done
  fi
  sleep "${WOLOS_E2E_BOOT_S:-150}"
  w screenshot "$n" "$d/booted.png" >/dev/null 2>&1 || :
  w stop "$n" --force >/dev/null 2>&1
  w serial "$n" tail -n 1000000 > "$d/boot.log"
  to_raw "$n" "$d"
}

adjtime() { # dir: the installed slot's /etc/adjtime (empty when it has none) in $dir/adjtime
  local m=$1/mnt p; mkdir -p "$m"; : > "$1/adjtime"
  # shellcheck disable=SC2317,SC2329  # run through with_loop, by name
  read_adjtime() {
    for p in "${LOOP}"p*; do
      sudo mount -o ro,subvol=@wolos-a "$p" "$m" 2>/dev/null || continue
      # shellcheck disable=SC2024  # the copy is the caller's file on purpose; only reading needs root
      [ ! -f "$m/etc/adjtime" ] || sudo cat "$m/etc/adjtime" > "$1/adjtime"
      sudo umount "$m"
    done
  }
  with_loop "$1/disk.raw" read_adjtime "$1"
}

# slot_file DIR PATH OUT [SUBVOL]: PATH in the installed system's SUBVOL (default @wolos-a, the
# slot; /var is the shared @var, so a /var path is lib/... in @var), empty OUT when it has none.
slot_file() {
  local m=$1/mnt sv=${4:-@wolos-a}; mkdir -p "$m"; : > "$3"
  # shellcheck disable=SC2317,SC2329  # run through with_loop, by name
  read_slot_file() {
    local p
    for p in "${LOOP}"p*; do
      sudo mount -o "ro,subvol=$sv" "$p" "$m" 2>/dev/null || continue
      # shellcheck disable=SC2024  # the copy is the caller's file on purpose; only reading needs root
      [ ! -f "$m/$2" ] || sudo cat "$m/$2" > "$3"
      sudo umount "$m"
    done
  }
  with_loop "$1/disk.raw" read_slot_file "$1" "$2" "$3"
}

# serve_repo DIR: $WOLOS_E2E_REPO over HTTP on a free port; prints "PID PORT".
serve_repo() {
  local log=$1/repo-http.log pid port t=0
  (cd "$WOLOS_E2E_REPO" && exec python3 -u -m http.server 0 --bind 0.0.0.0) > "$log" 2>&1 &
  pid=$!
  until port=$(sed -n 's/.* port \([0-9]*\) .*/\1/p' "$log" | head -n 1); [ -n "$port" ] || [ "$t" -ge 20 ]; do
    sleep 1; t=$((t + 1))
  done
  [ -n "$port" ] || { kill "$pid" 2>/dev/null; return 1; }
  echo "$pid $port"
}

# repo_in_guest VM PORT: an offline start of the installed system to put the served repo first in
# its WolOS mirror list (the guest reaches this machine at 10.0.2.2) and to trust
# $WOLOS_E2E_REPO_KEY, the build's own signing key; then the VM is off again.
repo_in_guest() {
  local n=$1 port=$2 key='' fpr='' rc=0
  if [ -n "${WOLOS_E2E_REPO_KEY:-}" ]; then
    key=$(base64 -w0 "$WOLOS_E2E_REPO_KEY") || return 1
    fpr=$(gpg --with-colons --show-keys "$WOLOS_E2E_REPO_KEY" | awk -F: '$1 == "fpr" { print $10; exit }')
    [ -n "$fpr" ] || return 1
  fi
  w start "$n" >/dev/null || return 1
  w wait-agent "$n" --timeout "${WOLOS_E2E_BOOT_S:-150}" >/dev/null || rc=1
  [ "$rc" = 1 ] || w ssh "$n" "sed -i '1i Server = http://10.0.2.2:$port' /etc/pacman.d/wolos-mirrorlist && sync" >/dev/null 2>&1 || rc=1
  if [ "$rc" = 0 ] && [ -n "$key" ]; then
    w ssh "$n" "echo $key | base64 -d > /run/e2e-key.asc && pacman-key --add /run/e2e-key.asc && pacman-key --lsign-key $fpr && sync" >/dev/null 2>&1 || rc=1
  fi
  w stop "$n" >/dev/null 2>&1 || :
  wait_off "$n" 120
  return "$rc"
}

# offline_modules DIR: installed with no network and a module ticked (audit6 I6-6). The module waits
# in modules-pending, the offline boot leaves it there (no tries used up, I6-3), and the first
# start WITH a network installs it and empties the file (firstboot / 50-wolos-online, I6-2).
# The module is the one install_scenario ticked (the smallest the Modules page offers), named in
# the install log as "e2e-module: <id>". No install, no system: nothing after that is checked.
offline_modules() {
  local d=$1 n=e2e-offline-modules t=0 q=${WOLOS_E2E_MODULES_S:-900} id
  id=$(grep -a 'e2e-module: ' "$d/install.log" | sed 's/.*e2e-module: \([a-z0-9-]*\).*/\1/' | head -n 1)
  if [ -z "$id" ] || ! grep -aq 'wolos-install: done' "$d/install.log"; then
    echo "E2E FAIL $S: no module ticked and installed (install log)"; return
  fi
  slot_file "$d" lib/wolos/modules-pending "$d/pending" @var
  say "the module was queued at install (no network)" "grep -qx '$id' '$d/pending'"
  slot_file "$d" lib/wolos/modules-attempts "$d/attempts" @var
  say "the offline start counted no try" "[ ! -s '$d/attempts' ]"
  slot_file "$d" lib/wolos/modules-gave-up "$d/gave-up" @var
  say "nothing given up offline" "[ ! -s '$d/gave-up' ]"
  # A build not yet published (pre-release): its repo and signing key come from this machine. The
  # guest is told while still offline (its own start, guest agent), so the online start drains
  # through the real path: the connection coming up (50-wolos-online).
  local srv=''
  if [ -n "${WOLOS_E2E_REPO:-}" ]; then
    srv=$(serve_repo "$d") || { echo "E2E FAIL $S: could not serve $WOLOS_E2E_REPO"; return; }
    if ! repo_in_guest "$n" "${srv#* }"; then
      kill "${srv%% *}" 2>/dev/null; echo "E2E FAIL $S: could not point the guest at the local repo"; return
    fi
  fi
  w set "$n" --net on >/dev/null 2>&1 || { echo "E2E FAIL $S: wolvm set --net on failed"; return; }
  w start "$n" >/dev/null || { echo "E2E FAIL $S: the online start failed"; return; }
  until w ssh "$n" "test ! -e /var/lib/wolos/modules-pending && pacman -Q wolos-module-$id" >/dev/null 2>&1 || [ "$t" -ge "$q" ]; do
    sleep 10; t=$((t + 10))
  done
  w ssh "$n" "test ! -e /var/lib/wolos/modules-pending && echo pending-empty; pacman -Q wolos-module-$id; systemctl --failed --no-legend --plain; echo end" > "$d/online.txt" 2>&1 || :
  w stop "$n" --force >/dev/null 2>&1
  [ -z "$srv" ] || kill "${srv%% *}" 2>/dev/null
  say "the pending file emptied once online" "grep -qx pending-empty '$d/online.txt'"
  say "the module is installed" "grep -q '^wolos-module-$id ' '$d/online.txt'"
  say "no unit failed after the drain" "grep -qx end '$d/online.txt' && ! grep -q 'failed' '$d/online.txt'"
}

blessed() { # dir: the boot entry lost its counter
  local m=$1/mnt rc=1 p; mkdir -p "$m"
  # shellcheck disable=SC2317,SC2329  # run through with_loop, by name
  check_esp() {
    for p in "${LOOP}"p*; do
      sudo mount -o ro "$p" "$m" 2>/dev/null || continue
      if [ -d "$m/loader/entries" ]; then
        # shellcheck disable=SC2012  # entry names are ours (wolos-*.conf): plain names, one per line
        ls "$m/loader/entries" | tee "$1/entries"
        [ -e "$m/loader/entries/wolos-a.conf" ] && rc=0
      fi
      sudo umount "$m"
    done
  }
  with_loop "$1/disk.raw" check_esp "$1"
  return $rc
}

say() { if eval "$2"; then echo "E2E PASS $S: $1"; else echo "E2E FAIL $S: $1"; fi; }

untouched() { # the partitions this scenario must never write
  case $S in
    alongside-windows | encrypted-alongside) echo 1 2 ;;
    bitlocker-windows) echo 1 2 3 ;;   # refused: nothing at all is written
    alongside-linux) echo 2 4 ;;   # its startup partition is shared on purpose: see kept_checks
    replace) echo 3 4 ;;   # Replace takes partition 2 only: the home and the swap stay (audit6 I6-6)
    manual*) echo 4 ;;
  esac
}

kept_hashes() { # dir
  local n; for n in $(untouched); do part_hash "$1/disk.raw" "$n"; done
}

file_hash() { # dir partition fs-tool path-inside
  local d=$1 n=$2 mnt=$3 f=$4 m=$1/m
  read_file() {
    mkdir -p "$m"
    # shellcheck disable=SC2086  # $mnt is the mount command and its options ("ntfs-3g -o ro"), split on purpose
    sudo $mnt "${LOOP}p$n" "$m" && sudo sha256sum "$m/$f" | cut -d' ' -f1 > "$d/keep.after"
    sudo umount "$m"
  }
  with_loop "$d/disk.raw" read_file
}

kept_checks() { # dir hashes-before: what the person kept is still there
  local d=$1
  [ -z "$(untouched)" ] || say "partitions nobody chose byte-identical" "[ \"$2\" = \"\$(kept_hashes '$d')\" ]"
  case $S in
    alongside-windows | encrypted-alongside) file_hash "$d" 3 "ntfs-3g -o ro" Users/Alice/keep.bin ;;
    alongside-linux | replace | manual*)
      file_hash "$d" 1 "mount -o ro" EFI/fedora/grubx64.efi
      say "the other system's startup files intact" "cmp -s '$d/disk.raw.esp' '$d/keep.after'"
      file_hash "$d" 3 "mount -o ro" alice/keep.bin ;;
    *) return ;;
  esac
  say "the person's file intact" "cmp -s '$d/disk.raw.keep' '$d/keep.after'"
}

scenario() { # name
  S=$1; local d=$W/$S plan=$S before='' old_root=''
  rm -rf "$d"; mkdir -p "$d"
  case $S in
    erase|password|tpm) mk_disk blank "$d/disk.raw" ;;
    alongside-windows) mk_disk windows "$d/disk.raw"; plan=alongside ;;
    encrypted-alongside) mk_disk windows "$d/disk.raw" ;;
    bitlocker-windows) mk_disk bitlocker "$d/disk.raw"; plan=alongside ;;
    alongside-linux) mk_disk linux "$d/disk.raw"; plan=alongside ;;
    manual|manual-ext4|replace) mk_disk linux "$d/disk.raw" ;;
    offline-modules) mk_disk blank "$d/disk.raw"; plan=modules ;;
  esac
  before=$(kept_hashes "$d")
  [ "$S" != replace ] || old_root=$(part_hash "$d/disk.raw" 2)
  install_in_vm "$d" "$S" "$plan" || { echo "E2E FAIL $S: wolvm could not start the VM"; return; }
  grep -a 'summary:' "$d/install.log" | sed 's/.*summary: /  /' | sort -u
  if [ "$S" = bitlocker-windows ]; then
    # Refused before any write (the Windows side has to turn BitLocker off first), and says why.
    say "BitLocker disk refused (no install)" "! grep -aq 'wolos-install: done' '$d/install.log'"
    say "the refusal names BitLocker" "grep -aqi 'bitlocker' '$d/install.log'"
    kept_checks "$d" "$before"
    return
  fi
  say "installer said done" "grep -aq 'wolos-install: done' '$d/install.log'"
  kept_checks "$d" "$before"
  boot_in_vm "$d" "$S"
  say "booted (entry blessed)" "blessed '$d'"
  # The boot's seal check passed: a failed one leaves verity-failed-<slot> in /var (@var).
  slot_file "$d" lib/wolos/verity-failed-wolos-a "$d/verity-failed" @var
  say "the seal check passed (no verity-failed mark)" "[ ! -s '$d/verity-failed' ]"
  # Windows stays: the hardware clock in local time (install-run, audit3 R2B-12); else UTC.
  case $S in
    alongside-windows) adjtime "$d"; say "clock in local time (/etc/adjtime LOCAL)" "grep -qx LOCAL '$d/adjtime'" ;;
    alongside-linux | replace) adjtime "$d"; say "clock in UTC (no Windows)" "! grep -qx LOCAL '$d/adjtime'" ;;
  esac
  # Replace on a non-WolOS Linux (audit6 I6-6): the old root is now WolOS (rewritten), the other
  # system's startup files, home and swap are as they were (kept_checks), and one WolOS entry.
  if [ "$S" = replace ]; then
    say "the replaced partition was rewritten" "[ \"$old_root\" != \"\$(part_hash '$d/disk.raw' 2)\" ]"
    say "one WolOS entry, none from the replaced system" "[ \"\$(grep -c '^wolos-' '$d/entries')\" = 1 ]"
  fi
  [ "$S" != offline-modules ] || offline_modules "$d"
  [ "$S" != manual-ext4 ] || say "one copy, no second slot" "! grep -q wolos-b '$d/entries'"
}

layouts() { # bin_dir names...: the real partitioning on loop devices, no VM and no boot
  local bin=$1; shift
  for S in "$@"; do
    local d=$W/layout-$S plan=$S before
    case $S in
      # A whole-disk erase (also password and tpm, which erase too) goes through systemd-repart in
      # install-run, never through `wol install layout` ("Erase runs through systemd-repart"):
      # `run $S` covers them.
      erase | password | tpm) echo "E2E SKIP $S: erases the whole disk (systemd-repart in install-run, not wol install layout); see run $S"; continue ;;
    esac
    rm -rf "$d"; mkdir -p "$d"
    # The throw-away disk key install-run makes before the layout (an encrypted plan formats with it).
    (umask 077 && head -c 64 /dev/urandom > "$d/key") || { echo "E2E FAIL $S: no disk key"; continue; }
    case $S in
      alongside-windows) mk_disk windows "$d/disk.raw"; plan=alongside ;;
      encrypted-alongside) mk_disk windows "$d/disk.raw" ;;   # beside Windows, as in scenario()
      alongside-linux) mk_disk linux "$d/disk.raw"; plan=alongside ;;
      *) mk_disk linux "$d/disk.raw" ;;
    esac
    before=$(kept_hashes "$d")
    # shellcheck disable=SC2024  # the output files are the caller's on purpose; only wol needs root
    apply_layout() {
      sudo "$bin/wol" install probe "$LOOP" > "$d/probe.json" || return 1
      "$bin/install_scenario" "$plan" "$d/probe.json" "$d/plan" || return 1
      sudo "$bin/wol" install layout "$d/plan/plan.json" "$d/key" > "$d/layout.env"; local rc=$? m
      # An encrypted plan leaves its volumes open (install-run closes them later): close them here,
      # or the loop device stays busy and the next encrypted layout finds wolosroot taken.
      for m in $(sed -n 's|^[a-z_]*=/dev/mapper/||p' "$d/layout.env"); do sudo cryptsetup close "$m"; done
      return $rc
    }
    if ! with_loop "$d/disk.raw" apply_layout; then echo "E2E FAIL $S: layout not applied"; continue; fi
    echo "E2E PASS $S: layout applied"
    grep -v '^extra_fstab=$' "$d/layout.env"
    sfdisk -d "$d/disk.raw"
    kept_checks "$d" "$before"
  done
}

boot_files() { # dir: the sha256 of every file under EFI/wolos on the disk, in $dir/boot-files
  local m=$1/mnt p; mkdir -p "$m"; : > "$1/boot-files"
  # shellcheck disable=SC2317,SC2329  # run through with_loop, by name
  list_boot() {
    for p in "${LOOP}"p*; do
      sudo mount -o ro "$p" "$m" 2>/dev/null || continue
      [ ! -d "$m/EFI/wolos" ] || (cd "$m" && sudo find EFI/wolos -type f -exec sha256sum {} +) | sort >> "$1/boot-files"
      sudo umount "$m"
    done
  }
  with_loop "$1/disk.raw" list_boot "$1"
}

windows_hashes() { # disk.raw: the Windows partitions (system, reserved, Windows) as they are
  local n; for n in 1 2 3; do part_hash "$1" "$n"; done
}

reinstall() { # WolOS again beside Windows, over the old WolOS (audit3 R2B-5, audit5 I6-7)
  # alongside-windows first; then Replace on the old WolOS partition: its startup files are
  # replaced (install-run clears EFI/wolos when no WolOS stays), Windows stays byte-identical,
  # the clock stays local, the disk boots with one WolOS entry.
  S=reinstall-windows; local d=$W/$S before
  rm -rf "$d"; mkdir -p "$d"
  mk_disk windows "$d/disk.raw"
  install_in_vm "$d" "$S" alongside || { echo "E2E FAIL $S: wolvm could not start the VM"; return; }
  say "first install (alongside) said done" "grep -aq 'wolos-install: done' '$d/install.log'"
  boot_in_vm "$d" "$S"
  say "first install booted (entry blessed)" "blessed '$d'"
  boot_files "$d"; mv "$d/boot-files" "$d/boot-files.1"
  mv "$d/install.log" "$d/install-1.log"; mv "$d/boot.log" "$d/boot-1.log"
  # install_scenario's replace-wolos picks the old WolOS in Replace's list the way the installer
  # knows it (its GPT name or file system label WolOS); the disk goes in as the first install left it.
  before=$(windows_hashes "$d/disk.raw")
  install_in_vm "$d" "$S" replace-wolos || { echo "E2E FAIL $S: wolvm could not start the VM again"; return; }
  grep -a 'summary:' "$d/install.log" | sed 's/.*summary: /  /' | sort -u
  say "second install (replace) said done" "grep -aq 'wolos-install: done' '$d/install.log'"
  say "Windows partitions byte-identical" "[ \"$before\" = \"\$(windows_hashes '$d/disk.raw')\" ]"
  file_hash "$d" 3 "ntfs-3g -o ro" Users/Alice/keep.bin
  say "the person's file intact" "cmp -s '$d/disk.raw.keep' '$d/keep.after'"
  boot_in_vm "$d" "$S"
  say "booted again (entry blessed)" "blessed '$d'"
  boot_files "$d"
  say "the old WolOS startup files replaced" "[ -s '$d/boot-files' ] && ! cmp -s '$d/boot-files.1' '$d/boot-files'"
  say "one WolOS entry, none left from the old install" "[ \"\$(grep -c '^wolos-' '$d/entries')\" = 1 ]"
  adjtime "$d"; say "clock in local time (/etc/adjtime LOCAL)" "grep -qx LOCAL '$d/adjtime'"
}

medium() { # the installer's own medium is refused
  S=medium; local d=$W/$S n=e2e-medium; rm -rf "$d"; mkdir -p "$d"
  mk_disk blank "$d/disk.raw"
  # The ISO as a writable disk (a USB stick; writes go to a throwaway overlay), not a CD.
  w new "$n" --from-image "$d/disk.raw" --disk "$(stat -c %s "$d/disk.raw")" --iso "$ISO" --iso-bus virtio --direct --no-net \
    --mem 6G --smp 4 --gpu virtio --append "wolos.e2e=medium wolos.try quiet audit=0" --force >/dev/null &&
    w start "$n" >/dev/null && wait_off "$n" 300
  w serial "$n" tail -n 1000000 > "$d/install.log"
  say "boot medium refused" "grep -aq 'medium-refused' '$d/install.log'"
}

case ${1:-} in
  remaster) shift; remaster "$@" ;;
  layouts) shift
    [ $# -gt 1 ] || { echo "usage: $0 layouts BIN_DIR NAME..." >&2; exit 2; }
    # A verdict like run's: any E2E FAIL, or no E2E PASS at all (nothing ran, or every name was
    # skipped), is a failed run.
    layouts "$@" 2>&1 | tee "$W/layouts.result"
    if ! grep -q '^E2E PASS' "$W/layouts.result" && grep -q '^E2E SKIP' "$W/layouts.result"; then
      echo "layouts: every name was skipped, no layout ran: name one wol install layout applies (not erase, password or tpm)" >&2
    fi
    sh "$REPO/tests/test/verdict.sh" E2E "$W/layouts.result" ;;
  run) shift
    for s in "$@"; do
      rm -f "$W/$s.result"
      case $s in
        medium) medium ;;
        reinstall-windows) reinstall ;;
        *) scenario "$s" ;;
      esac 2>&1 | tee "$W/$s.result" &
    done
    wait
    [ $# -gt 0 ] || { echo "usage: $0 run SCENARIO..." >&2; exit 2; }
    # This run's results only (an older *.result in $W is not this run's).
    res=''
    for s in "$@"; do grep -h '^E2E\|install took' "$W/$s.result" 2>/dev/null; res="$res $W/$s.result"; done
    # shellcheck disable=SC2086  # one word per result file
    sh "$REPO/tests/test/verdict.sh" E2E $res ;;
  *) echo "usage: $0 remaster BASE.iso BIN_DIR | run SCENARIO..." >&2; exit 2 ;;
esac
