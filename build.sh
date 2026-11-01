#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Build the WolOS live + installer ISO (x86_64, UEFI) with mkarchiso from profile/ (ADR 0016), from
# the published [wolos] repo and the Arch snapshot only: no source tree, no package build here
# (ADR "repository split"). Run by `wolos-build iso` (locally or on the ECS cluster).
# Host: Arch (or CachyOS) with archiso; without mkarchiso it runs in an archlinux podman container.
# Steps: staged profile in $WORK/profile (the repo + its key) -> mkarchiso -> ISO + release manifest.
# Env: WOLOS_REPO_NAME (default wolos-testing; wolos = stable), WOLOS_REPO_URL (default its GitHub
#      release in wonderlandos/wolos-packages), WOLOS_WORK (default ~/WolOS-vm/build),
#      WOLOS_OUT (default $WORK/out), WOLOS_STAGE_ONLY=1 (stage the profile, no ISO),
#      WOLOS_IMAGE_URL (manifest base URL), WOLOS_CACHY_DIR ([wolos-cachy], its revision),
#      WOLOS_SB_LAYOUT=1 (shim layout), WOLOS_PODMAN=1 (force the container).
#      Disk: ~15 GB in $WORK (packages + squashfs + ISO).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${WOLOS_WORK:-${WOLOS_VM:-$HOME/WolOS-vm}/build}
OUT=${WOLOS_OUT:-$WORK/out}
SNAP=$(tr -d '[:space:]' < "$HERE/profile/snapshot")
NAME=${WOLOS_REPO_NAME:-wolos-testing}
case $NAME in wolos) tag=wolos-$SNAP ;; *) tag=$NAME ;; esac
URL=${WOLOS_REPO_URL:-https://github.com/wonderlandos/wolos-packages/releases/download/$tag}
P=$WORK/profile
die() { echo "build.sh: $*" >&2; exit 1; }
mkdir -p "$WORK" "$OUT"

# 1. The repo as published: its database (versions for the manifest) and its revision.
DB=$WORK/$NAME.db
curl -fsSL --retry 5 -o "$DB" "$URL/$NAME.db" || die "no [$NAME] database at $URL"
REV=$(curl -fsSL --retry 5 "$URL/revision" 2>/dev/null || echo unknown)
# field PKG FIELD: %FIELD% of package PKG in the database (each desc starts with %FILENAME%).
field() { bsdtar -xOf "$DB" '*/desc' | awk -v p="$1" -v f="$2" '
  function flush() { if (rec["NAME"] == p && (f in rec)) out = rec[f]; delete rec }
  /^%[A-Z0-9]+%$/ { k = substr($0, 2, length($0) - 2); if (k == "FILENAME") flush(); getline; rec[k] = $0; next }
  END { flush(); print out }'; }
VER=$(field wolos-base VERSION); VER=${VER%-*}
KV=$(field linux-wolos VERSION)
[ -n "$VER" ] && [ -n "$KV" ] || die "[$NAME] at $URL has no wolos-base or linux-wolos"

# 2. Stage the profile: base + desktop packages, [wolos] from the published repo for this build,
# signatures checked against the WolOS key (release/wolos-repo.pub.asc) in a keyring of its own.
rm -rf "$P"; cp -a "$HERE/profile" "$P"
cat "$HERE/profile/packages.x86_64" "$HERE/profile/packages.desktop" | sed 's/#.*//' | tr -s ' \t' '\n' | grep . > "$P/packages.x86_64"
rm -f "$P/packages.desktop" "$P/packages-base.txt"
G=$WORK/gnupg; S=; [ "$(id -u)" = 0 ] || S=sudo
$S rm -rf "$G"; $S cp -a /etc/pacman.d/gnupg "$G"
$S pacman-key --gpgdir "$G" --add "$HERE/release/wolos-repo.pub.asc" >/dev/null
for fpr in $(gpg --show-keys --with-colons "$HERE/release/wolos-repo.pub.asc" | awk -F: '/^fpr/ {print $10}'); do
  $S pacman-key --gpgdir "$G" --lsign-key "$fpr" >/dev/null
done
# No DBPath for the build: pacstrap writes the database to its usual place on the build host, and
# install-run moves it into the slot (/usr/lib/pacman, as airootfs/etc/pacman.conf says).
awk -v name="[$NAME]" -v srv="Server = $URL" -v gpg="GPGDir = $G/" '
  /^DBPath/ { next }
  /^\[options\]/ { print; print gpg; next }
  /^\[/ { sect = $0 }
  sect == "[wolos]" && /^\[wolos\]/ { print name; next }
  sect == "[wolos]" && /^(Server|Include) = / { print srv; next }
  { print }' "$HERE/profile/pacman.conf" > "$P/pacman.conf"
A=$P/airootfs
mkdir -p "$A/etc/wolos" "$A/etc/sysusers.d" "$A/etc/sudoers.d" "$A/etc/tmpfiles.d"
echo "$SNAP" > "$A/etc/wolos/snapshot"
# Live desktop (ADR 0009): `live` logs in automatically; it may run the installer as root, no password.
cat > "$A/etc/sysusers.d/wolos-live.conf" <<'EOF'
u live 1000 "WolOS Live" /home/live /usr/bin/zsh
m live wheel
m live video
m live input
m live seat
EOF
echo 'd /home/live 0700 live live -' > "$A/etc/tmpfiles.d/wolos-live.conf"
echo 'live ALL=(root) NOPASSWD: /usr/lib/wolos/install-run, /usr/lib/wolos/install-check, /usr/bin/systemctl reboot, /usr/bin/systemctl poweroff' > "$A/etc/sudoers.d/wolos-live"
printf '%s\n' '{"autologin":"live"}' > "$A/etc/wolos/login.json"
touch "$A/etc/wolos-live"
if [ "${WOLOS_STAGE_ONLY:-0}" = 1 ]; then echo "profile: $P"; exit 0; fi

# A killed mkarchiso leaves pacstrap's mounts (devtmpfs, proc, sys, efivarfs) in the work dir:
# unmount them first, and never let rm cross into another file system (the host's /dev).
clean_work() {
  local s=; [ "$(id -u)" = 0 ] || s=sudo
  [ -e "$WORK/work" ] || return 0
  findmnt -rn -o TARGET | awk -v w="$WORK/work/" 'index($0, w) == 1' | sort -r |
    while read -r m; do $s umount -l "$m"; done
  $s rm -rf --one-file-system "$WORK/work"
}

# 3. mkarchiso (root) on the host, or in a container.
clean_work
if command -v mkarchiso >/dev/null && [ "${WOLOS_PODMAN:-0}" != 1 ]; then
  S=; [ "$(id -u)" = 0 ] || S=sudo
  $S mkarchiso -v -w "$WORK/work" -o "$OUT" "$P"
else
  command -v podman >/dev/null || die "needs mkarchiso (pacman -S archiso) or podman"
  podman run --rm --privileged -v "$WORK:$WORK" -v "$OUT:$OUT" archlinux:base-devel \
    bash -c "pacman -Sy --noconfirm archiso >/dev/null && mkarchiso -v -w '$WORK/work' -o '$OUT' '$P'"
fi
clean_work   # disk: the pacstrap root is in the squashfs now
# Secure Boot layout (ADR 0025, release/sb-layout.sh): shim, then systemd-boot, then one UKI. Off
# until the WolOS Secure Boot key exists; release.yml signs it with WOLOS_SB_KEY / WOLOS_SB_CERT.
# The shim comes from the repo the image was built from.
if [ "${WOLOS_SB_LAYOUT:-0}" = 1 ]; then
  mkdir -p "$WORK/shim/x86_64"
  f=$(field wolos-shim FILENAME)
  curl -fsSL --retry 5 -o "$WORK/shim/x86_64/$f" "$URL/$f"
  $S "$HERE/release/sb-layout.sh" layout "$(ls -t "$OUT"/wolos-*.iso | head -1)" "$WORK/shim"
fi

# 4. Release manifest next to the ISO.
ISO=$(ls -t "$OUT"/wolos-*.iso | head -1)
"$HERE/release/release-manifest.sh" "$ISO" "$SNAP" "$REV" "${WOLOS_IMAGE_URL:-https://github.com/wonderlandos/wolos-packages/releases/download/$SNAP}" \
  "$VER" "$KV" "$(cat "${WOLOS_CACHY_DIR:-$WORK/cachy}/revision" 2>/dev/null || :)" > "${ISO%.iso}.json"
# A release asset is at most 2 GiB: the ISO is one download, never split.
SZ=$(stat -c %s "$ISO")
[ "$SZ" -le $((2047 * 1024 * 1024)) ] || die "$ISO is $SZ bytes, over the 2047 MiB release asset limit"
echo "image: $ISO  manifest: ${ISO%.iso}.json"
