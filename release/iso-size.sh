#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# iso-size.sh ISO: print the image's size. Never fails on it: the image has no size limit (ISO-GATE,
# lead 2026-11-04; the 2047 MiB gate was the CTO's, not the lead's). GitHub takes no release asset of
# 2 GiB or more; `wolos-build release` then puts the ISO on the CDN only and links it from the GitHub
# release, so the note below is information, not a verdict.
set -eu
iso=${1:?ISO}
sz=$(stat -c %s "$iso")
gib=$(awk -v s="$sz" 'BEGIN { printf "%.1f", s / 1073741824 }')
echo "image size: $sz bytes ($gib GiB)"
if [ "$sz" -ge 2147483648 ]; then
  echo "note: over GitHub's 2 GiB asset size: the ISO is published on the CDN (cdn.wolos.org) only; the GitHub release links it"
fi
