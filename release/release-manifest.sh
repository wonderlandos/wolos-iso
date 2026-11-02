#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Release manifest: ISO SNAPSHOT(YYYY-MM-DD) REPO_REVISION BASE_URL VERSION KERNEL -> release.json
# on stdout, the form `wol update` reads (wol_core::update::check::Manifest): version, snapshot
# (YYYY/MM/DD, the Arch Linux Archive path), wolos_repo, kernel, image URL, sha256, size. No `cachy`
# field since ADR 0030 (the CachyOS repos are not republished; Manifest defaults it to "").
# VERSION (wolos-base's) and KERNEL (linux-wolos's full version) come from the repo the image was
# built from (build.sh reads its database): the image names what it ships, not a source tree.
set -euo pipefail
ISO=${1:?ISO}; SNAP=${2:?SNAPSHOT}; REV=${3:?REVISION}; URL=${4:?BASE_URL}
VER=${5:?VERSION}; KV=${6:?KERNEL}
sum=$(sha256sum "$ISO" | cut -d' ' -f1)
size=$(stat -c %s "$ISO")
printf '{"version":"%s","snapshot":"%s","wolos_repo":"%s","kernel":"%s","image":"%s/%s","sha256":"%s","size":%s}\n' \
  "$VER" "${SNAP//-//}" "$REV" "$KV" "${URL%/}" "$(basename "$ISO")" "$sum" "$size"
