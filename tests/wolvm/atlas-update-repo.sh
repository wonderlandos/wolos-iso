#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Host side of journeys/atlas-update.yaml: a local pacman repo with two versions of atlas-browser,
# served to the guest at http://10.0.2.2:$PORT/n/ (version N) and /n1/ (version N+1).
#   tests/wolvm/atlas-update-repo.sh WORKDIR PKGBUILDS-ATLAS-BROWSER-DIR TARBALL-N TARBALL-N1 [PORT]
# On an Arch host (makepkg, repo-add, python3). Each tarball is a lane artifact named
# atlas-VERSION-x86_64_linux.tar.xz (the lane's package step); the recipe is repackaged as it is,
# nothing is signed (the guest repo says SigLevel = Never). VERSION may differ between the two
# (then the browser's own version changes) or be the same (then N1 is pkgrel 2).
# Writes WORKDIR/{n,n1}/ (packages + atlas-test.db) and WORKDIR/{n,n1}.txt: the pacman version and
# the first line of `atlas --version` of each tarball, which the journey compares in the guest.
# Serves WORKDIR until interrupted.
set -euo pipefail
work=${1:?WORKDIR} recipe=${2:?recipe dir} tn=${3:?tarball N} tn1=${4:?tarball N+1} port=${5:-8015}
mkdir -p "$work"; work=$(cd "$work" && pwd); recipe=$(cd "$recipe" && pwd)
build() { # LABEL TARBALL PKGREL
  local label=$1 tar=$2 rel=$3 ver b="$work/b-$1" out="$work/$1" x="$work/x-$1"
  ver=$(basename "$tar"); ver=${ver#atlas-}; ver=${ver%%-x86_64_linux*}
  rm -rf "$b" "$out" "$x"; mkdir -p "$b" "$out" "$x"; cp -a "$recipe/." "$b/"
  sed -i "s/^pkgver=.*/pkgver=$ver/; s/^pkgrel=.*/pkgrel=$rel/" "$b/PKGBUILD"
  mkdir -p "$work/src"; cp -f "$tar" "$work/src/atlas-$ver-x86_64_linux.tar.xz"
  : > "$work/src/atlas-$ver-x86_64_linux.tar.xz.asc"
  (cd "$b" && SRCDEST="$work/src" PKGDEST="$out" BUILDDIR="$work/build" \
    makepkg --nodeps --skippgpcheck --skipchecksums -f --noconfirm >"$work/$label.log" 2>&1)
  repo-add -q "$out/atlas-test.db.tar.zst" "$out"/*.pkg.tar.zst
  ln -sf atlas-test.db.tar.zst "$out/atlas-test.db"
  bsdtar -xf "$out/atlas-browser-$ver-$rel-x86_64.pkg.tar.zst" -C "$x" opt
  printf '%s-%s\n' "$ver" "$rel" > "$work/$label.txt"
  env -i HOME="$x" "$x/opt/atlas/atlas" --version 2>/dev/null | head -1 >> "$work/$label.txt" || true
}
build n "$tn" 1
n_ver=$(sed -n 1p "$work/n.txt")
if [ "$(basename "$tn1")" = "$(basename "$tn")" ]; then rel=2; else rel=1; fi
build n1 "$tn1" "$rel"
[ "$(sed -n 1p "$work/n1.txt")" != "$n_ver" ] || { echo "N and N+1 have the same version" >&2; exit 1; }
echo "N: $(tr '\n' ' ' < "$work/n.txt") / N+1: $(tr '\n' ' ' < "$work/n1.txt")"
echo "serving $work on :$port (guest: http://10.0.2.2:$port/n/ and /n1/)"
cd "$work" && exec python3 -m http.server "$port" --bind 0.0.0.0
