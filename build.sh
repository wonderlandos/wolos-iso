#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Build the WolOS live + installer ISO (x86_64, UEFI) with mkarchiso from profile/ (ADR 0016), from
# the published [wolos] repo and the Arch snapshot only: no source tree, no package build here
# (ADR "repository split"). The one build path: `wolos-build iso` (the forge or `--ecs`) runs this;
# by hand on an Arch host with archiso it is the same script.
# Steps: the repo's snapshot must equal profile/snapshot -> staged profile in $WORK/profile (the repo,
# its key, the channel the installed system follows) -> mkarchiso -> ISO + <iso>.sha256 + manifest.
# Env: WOLOS_REPO_NAME (default wolos-testing = the testing channel; wolos = stable), WOLOS_REPO_URL
#      (default its GitHub release in wonderlandos/wolos-packages), WOLOS_VM (default ~/WolOS-vm),
#      WOLOS_WORK (default $WOLOS_VM/build), WOLOS_OUT (default $WOLOS_VM/out, where tests/*.sh
#      look), WOLOS_STAGE_ONLY=1 (stage the profile, no ISO), WOLOS_IMAGE_URL (manifest base URL),
#      WOLOS_SB_LAYOUT=1 (shim layout), WOLOS_NEED_MB (free disk needed in $WORK, default 20000),
#      WOLOS_SNAPSHOT_CHECK=0 (build from a repo that publishes no `snapshot` file; never a release).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
VM=${WOLOS_VM:-$HOME/WolOS-vm}
WORK=${WOLOS_WORK:-$VM/build}
OUT=${WOLOS_OUT:-$VM/out}
SNAP=$(tr -d '[:space:]' < "$HERE/profile/snapshot")
NAME=${WOLOS_REPO_NAME:-wolos-testing}
case $NAME in wolos) tag=wolos-$SNAP ;; *) tag=$NAME ;; esac
URL=${WOLOS_REPO_URL:-https://github.com/wonderlandos/wolos-packages/releases/download/$tag}
P=$WORK/profile
die() { echo "build.sh: $*" >&2; exit 1; }
mkdir -p "$WORK" "$OUT"
# A full disk paused a build mid-way once and it died silently: refuse up front (packages,
# squashfs and ISO take ~15 GB). Staging alone needs next to nothing.
if [ "${WOLOS_STAGE_ONLY:-0}" != 1 ]; then
  free=$(df -Pm "$WORK" | awk 'NR == 2 {print $4}'); need=${WOLOS_NEED_MB:-20000}
  if [ "$free" -lt "$need" ]; then
    echo "build.sh: host has $free MB free in $WORK, needs $need MB (trim $VM/build, $OUT)" >&2
    exit 2
  fi
fi

# 1. The repo as published: its database (versions for the manifest), its revision and the Arch
# snapshot its packages were built against (`wolos-build publish` writes it). An image pinned to
# another date than its packages installs a mix nobody tested: refuse it.
DB=$WORK/$NAME.db
curl -fsSL --retry 5 -o "$DB" "$URL/$NAME.db" || die "no [$NAME] database at $URL"
REV=$(curl -fsSL --retry 5 "$URL/revision" 2>/dev/null || echo unknown)
if [ "${WOLOS_SNAPSHOT_CHECK:-1}" != 0 ]; then
  RSNAP=$(curl -fsSL --retry 5 "$URL/snapshot" 2>/dev/null | tr -d '[:space:]') ||
    die "no snapshot file at $URL/snapshot: the repo was not published by \`wolos-build publish\` (re-publish it, or WOLOS_SNAPSHOT_CHECK=0 for a test image)"
  [ "$RSNAP" = "$SNAP" ] ||
    die "[$NAME] at $URL was built against Arch snapshot $RSNAP, profile/snapshot says $SNAP: bump one of them (wolos-pkgbuilds tools/bump-snapshot.sh) and rebuild"
fi
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
rm -f "$P/packages.desktop"
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
# The channel (audit CA-3): the live root's WolOS section names the repo and release tag this image
# was built from ([wolos-testing] or [wolos] + `Server = URL`); install-run's install-channel turns
# it back into the mirrorlist on the installed system, with that tag, so a testing image installs a
# system that follows testing and a stable one the stable tag.
awk -v name="[$NAME]" -v srv="Server = $URL" '
  /^\[/ { sect = $0 }
  sect == "[wolos]" && /^\[wolos\]/ { print name; next }
  sect == "[wolos]" && /^(Server|Include) = / { if (!done) print srv; done = 1; next }
  { print }' "$HERE/profile/airootfs/etc/pacman.conf" > "$A/etc/pacman.conf"
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

# 3. mkarchiso (root) on the host: one path, no container fallback (the forge and the ECS builder
# image have archiso).
command -v mkarchiso >/dev/null || die "needs mkarchiso (pacman -S archiso), or run \`wolos-build iso\` on the forge"
clean_work
S=; [ "$(id -u)" = 0 ] || S=sudo
$S mkarchiso -v -w "$WORK/work" -o "$OUT" "$P"
clean_work   # disk: the pacstrap root is in the squashfs now
# Secure Boot (ADR 0025, release/sb-layout.sh): shim, then systemd-boot, then one UKI, both signed
# with the WolOS live-image key, so the medium starts with Secure Boot on (one blue key screen,
# once) and off (nothing changes). `wolos-build iso` sets WOLOS_SB_LAYOUT=1 where release/sb-key.sh
# finds the key (the forge: /etc/wolos-forge/sb). The shim comes from the repo the image was built
# from.
if [ "${WOLOS_SB_LAYOUT:-0}" = 1 ]; then
  if k=$("$HERE/release/sb-key.sh" path 2>/dev/null); then
    WOLOS_SB_KEY=$k WOLOS_SB_CERT=$("$HERE/release/sb-key.sh" cert)
  else
    WOLOS_SB_KEY=
  fi
  # The layout without a signature is an image shim refuses wherever Secure Boot is on: the
  # person's first boot would be the firmware's error screen. No key, no layout (with only
  # WOLOS_SB_CERT, release.yml signs later with its secrets).
  [ -n "${WOLOS_SB_CERT:-}" ] || die "WOLOS_SB_LAYOUT=1 needs the live-image key (release/sb-key.sh path) or WOLOS_SB_CERT: unsigned, the image would not boot with Secure Boot on"
  mkdir -p "$WORK/shim/x86_64"
  f=$(field wolos-shim FILENAME)
  curl -fsSL --retry 5 -o "$WORK/shim/x86_64/$f" "$URL/$f"
  ISO=$(ls -t "$OUT"/wolos-*.iso | head -1)
  $S "$HERE/release/sb-layout.sh" layout "$ISO" "$WORK/shim"
  if [ -n "$WOLOS_SB_KEY" ]; then
    $S "$HERE/release/sb-layout.sh" sign "$ISO" "$WOLOS_SB_KEY" "$WOLOS_SB_CERT"
    $S chown "$(id -u):$(id -g)" "$ISO"
    # A WOLOS_SB_CERT of its own (dev/test key) is what this build pins; `wolos-build release`
    # checks again against the WolOS live key and refuses such a medium.
    WOLOS_SB_PIN_CERT=$WOLOS_SB_CERT "$HERE/release/sb-layout.sh" check "$ISO" || die "$ISO: the Secure Boot layout does not check out (sb-layout.sh check)"
  fi
fi

# 4. The medium's own hash (plan W5): mkarchiso writes wolos/x86_64/airootfs.sha512 next to the
# squashfs ("<sha512>  airootfs.sfs"); the live wolos-medium-verify unit checks the stick against
# it (/run/archiso/bootmnt/wolos/x86_64). No file, no self-check: refuse the image.
ISO=$(ls -t "$OUT"/wolos-*.iso | head -1)
bsdtar -xOf "$ISO" wolos/x86_64/airootfs.sha512 2>/dev/null | grep -Eq '^[0-9a-f]{128}  airootfs\.sfs$' ||
  die "$ISO has no wolos/x86_64/airootfs.sha512 (mkarchiso's medium hash): the live medium check would have nothing to read"
# 5. The download's hash (README "Verify the download": sha256sum -c) and the release manifest.
(cd "$(dirname "$ISO")" && sha256sum "$(basename "$ISO")" > "$(basename "$ISO").sha256")
"$HERE/release/release-manifest.sh" "$ISO" "$SNAP" "$REV" "${WOLOS_IMAGE_URL:-https://github.com/wonderlandos/wolos-packages/releases/download/$SNAP}" \
  "$VER" "$KV" > "${ISO%.iso}.json"
# The size is reported, never a gate (ISO-GATE, lead 2026-11-04): an ISO over 2 GiB cannot be a GitHub
# asset, so `wolos-build release` publishes it on the CDN and links it from the GitHub release.
"$HERE/release/iso-size.sh" "$ISO"
echo "image: $ISO  sha256: $ISO.sha256  manifest: ${ISO%.iso}.json"
