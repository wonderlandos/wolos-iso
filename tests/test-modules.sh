#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# test-modules.sh: inside an installed, online WolOS (tests/run-installed.sh), prove the optional
# modules of ADR 0032 (wolos docs/modules/modules.md) end to end:
#  1. The registry lists every module and each meta package resolves in the repos (pacman -Sp,
#     through a temporary copy of the database: an installed slot keeps it on the read-only seal).
#  2. The root helper installs a module (ai-sessions: the smallest), and pacman, `wol modules status`
#     and `wol modules enable` (as the person, no password) agree on its state.
#  3. Queue and drain (installer offline -> firstboot): `modules-install --queue` writes
#     /var/lib/wolos/modules-pending, `--drain` installs it and empties the file.
#  4. ai-local's GPU backend follows `wol hw compute` (a VM has no GPU: cpu, no ollama-* backend).
#  5. The files the modules rely on are in place: Claude Code's managed settings and the WolOS skill
#     (only when the AI Assistants module, ai-claude, is installed), zellij, mise and the console
#     setup (a login zsh of the person has the WolOS history size), and `wol sessions list --json`
#     answers.
# Run as root with the desktop user as argument: sudo tests/test-modules.sh alice (a test VM with no
# account yet: tests/guest-account.sh makes one)
# Exit 0 and "test-modules: PASS", else the first failing step. A module this test installed is
# removed again on EXIT; one that was installed before stays.
set -euo pipefail
fail() { echo "test-modules: FAIL: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root (sudo $0 <user>)"
U=${1:-}
[ -n "$U" ] && id "$U" >/dev/null 2>&1 || fail "name the desktop user: $0 <user>"
for t in wol pacman runuser zsh; do command -v "$t" >/dev/null 2>&1 || fail "$t missing"; done
MI=/usr/lib/wolos/modules-install
P=/var/lib/wolos/modules-pending
[ -x "$MI" ] || fail "$MI missing"
as_user() { runuser -u "$U" -- env XDG_RUNTIME_DIR="/run/user/$(id -u "$U")" "$@"; }

M=ai-sessions
had=no
if pacman -Qq "wolos-module-$M" >/dev/null 2>&1; then had=yes; fi
cleanup() {
  [ "$had" = yes ] || pacman -R --noconfirm "wolos-module-$M" >/dev/null 2>&1 || :  # not installed: nothing to undo
  sed -i "/^$M\$/d" "$P" 2>/dev/null || :  # no pending file: nothing to undo
}
trap cleanup EXIT

# 1. Registry and repos agree.
J=$(as_user wol modules list --json) || fail "wol modules list --json"
# An installed slot keeps pacman's database on the read-only seal (ADR 0016): resolve through a
# temporary copy with the current repo lists, the way modules-install does.
db=$(pacman-conf DBPath 2>/dev/null || :); db=${db:-/var/lib/pacman}; db=${db%/}
tmpdb=$(mktemp -d /var/tmp/wolos-test-db.XXXXXX) && chmod 755 "$tmpdb" || fail "temporary database"
ln -s "$db/local" "$tmpdb/local"; [ ! -d "$db/sync" ] || cp -a "$db/sync" "$tmpdb/"
pacman --dbpath "$tmpdb" -Sy >/dev/null 2>&1 || fail "repo lists do not refresh"
for id in ai-claude ai-local ai-sessions dev creative gaming console; do
  echo "$J" | grep -q "\"id\": *\"$id\"" || fail "wol modules list lacks $id"
  pacman --dbpath "$tmpdb" -Sp --print-format %n "wolos-module-$id" >/dev/null || fail "wolos-module-$id does not resolve in the repos"
done
rm -rf "$tmpdb"

# 2. Install through the root helper; pacman, status and enable agree.
[ "$had" = yes ] || pacman -R --noconfirm "wolos-module-$M" >/dev/null 2>&1 || :  # not installed: fine
"$MI" "$M" || fail "modules-install $M"
pacman -Qq "wolos-module-$M" >/dev/null || fail "wolos-module-$M not installed"
as_user wol modules status "$M" --json | grep -q '"installed": *true' || fail "status does not say installed"
as_user wol modules enable "$M" </dev/null || fail "wol modules enable $M (must not ask for a password)"

# 3. Queue (offline installer), then drain (firstboot).
pacman -R --noconfirm "wolos-module-$M" >/dev/null || fail "could not remove $M for the drain test"
"$MI" --queue "$M" && fail "--queue must report the id as queued (exit 1)"
grep -qx "$M" "$P" || fail "$M not in $P"
as_user wol modules status "$M" --json | grep -q '"pending": *true' || fail "status does not say pending"
"$MI" --drain || fail "modules-install --drain"
pacman -Qq "wolos-module-$M" >/dev/null || fail "drain did not install $M"
if [ -f "$P" ] && grep -qx "$M" "$P"; then fail "$M still pending after the drain"; fi

# 4. GPU backend of the local-AI module: a VM is cpu.
C=$(as_user wol hw compute --json) || fail "wol hw compute --json"
echo "$C" | grep -q '"backend": *"\(cuda\|rocm\|vulkan\|cpu\)"' || fail "wol hw compute: $C"
if systemd-detect-virt -q; then
  echo "$C" | grep -q '"backend": *"cpu"' || fail "a VM must be cpu: $C"
fi

# 5. Files the modules rely on, and sessions.
for f in /etc/zellij/config.kdl /etc/mise/config.toml /usr/share/wolos/starship/starship.toml \
  /etc/profile.d/wolos-console.sh /etc/fish/conf.d/wolos.fish; do
  [ -r "$f" ] || fail "$f missing"
done
# Only a machine with the AI Assistants module has these (ADR 0032); a bare install has none.
if pacman -Qq wolos-module-ai-claude >/dev/null 2>&1; then
  for f in /etc/claude-code/managed-settings.json /usr/share/wolos/agents/skills/wolos/SKILL.md; do
    [ -r "$f" ] || fail "$f missing"
  done
  grep -q '"mcp__wolos"' /etc/claude-code/managed-settings.json || fail "managed settings do not allow wol mcp"
else
  echo "test-modules: ai-claude not installed, its files not checked"
fi
H=$(as_user zsh -lic 'print -r -- $HISTSIZE' 2>/dev/null | tail -1)
[ "$H" = 50000 ] || fail "a login zsh of $U has HISTSIZE=$H, not the WolOS 50000"
as_user wol sessions list --json | grep -q '"windows"' || fail "wol sessions list --json"

echo "test-modules: PASS"
