#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Release manifest: ISO SNAPSHOT(YYYY-MM-DD) REPO_REVISION BASE_URL VERSION KERNEL REPO -> release.json
# on stdout, the form `wol update` reads (wol_core::update::check::Manifest): version, snapshot
# (YYYY/MM/DD, the Arch Linux Archive path), wolos_repo, kernel, image URL, sha256, size. No `cachy`
# field since ADR 0030 (the CachyOS repos are not republished; Manifest defaults it to "").
# VERSION (wolos-base's) and KERNEL (linux-wolos's full version) come from the repo the image was
# built from (build.sh reads its database): the image names what it ships, not a source tree.
# REPO (`wolos` = stable, `wolos-testing`) is the repo that database belongs to: `wolos-build release`
# refuses a stable release whose image was not built from [wolos] (QA-A9-IMAGE-06) and leaves the
# field out of the published release.json.
set -euo pipefail
ISO=${1:?ISO}; SNAP=${2:?SNAPSHOT}; REV=${3:?REVISION}; URL=${4:?BASE_URL}
VER=${5:?VERSION}; KV=${6:?KERNEL}; REPO=${7:?REPO (wolos or wolos-testing)}
sum=$(sha256sum "$ISO" | cut -d' ' -f1)
size=$(stat -c %s "$ISO")
printf '{"version":"%s","repo":"%s","snapshot":"%s","wolos_repo":"%s","kernel":"%s","image":"%s/%s","sha256":"%s","size":%s}\n' \
  "$VER" "$REPO" "${SNAP//-//}" "$REV" "$KV" "${URL%/}" "$(basename "$ISO")" "$sum" "$size"
