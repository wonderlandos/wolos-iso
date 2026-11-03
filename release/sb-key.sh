#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# sb-key.sh: the WolOS live-image Secure Boot key (ADR 0025; wolos-build docs/signing-kms.md,
# "Secure Boot live key"). It signs the live medium only, never an installed system.
#   sb-key.sh path   the private key's path when this host can sign with it, else exit 3. The key
#                    is $WOLOS_SB_KEY when that names a file, else $WOLOS_SB_KEY_FILE (default
#                    /etc/wolos-forge/sb/live-2026.key: the forge, root 0600). Usable means readable
#                    here or by root through `sudo -n` (build.sh signs as root).
#   sb-key.sh cert   the certificate (PEM) that belongs to it: $WOLOS_SB_CERT when that names a
#                    file, else release/wolos-live-2026.crt next to this script
#   sb-key.sh name   the file name the medium carries the certificate under (wolos-live-2026)
# Only the path leaves this script: the key is never printed, copied or synced.
set -euo pipefail
HERE=$(cd "$(dirname "$(readlink -f "$0")")" && pwd)
NAME=wolos-live-2026 KEY=/etc/wolos-forge/sb/live-2026.key
case ${1:-} in
  path)
    k=${WOLOS_SB_KEY_FILE:-$KEY}
    if [ -n "${WOLOS_SB_KEY:-}" ] && [ -f "$WOLOS_SB_KEY" ]; then k=$WOLOS_SB_KEY; fi
    if [ -r "$k" ] && [ -f "$k" ]; then echo "$k"; exit 0; fi
    if [ "$(id -u)" != 0 ] && command -v sudo >/dev/null && sudo -n test -f "$k" 2>/dev/null &&
      sudo -n test -r "$k" 2>/dev/null; then echo "$k"; exit 0; fi
    echo "sb-key.sh: no Secure Boot live key at $k" >&2; exit 3 ;;
  cert)
    if [ -n "${WOLOS_SB_CERT:-}" ] && [ -f "$WOLOS_SB_CERT" ]; then echo "$WOLOS_SB_CERT"; else echo "$HERE/$NAME.crt"; fi ;;
  name) echo "$NAME" ;;
  *) echo "usage: $0 path | cert | name" >&2; exit 2 ;;
esac
