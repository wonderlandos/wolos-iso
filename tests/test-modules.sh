#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# test-modules.sh: inside an installed, online WolOS (tests/run-installed.sh), prove the optional
# modules of ADR 0032 (wolos docs/modules/modules.md) end to end:
#  1. The registry lists every module and each meta package resolves in the repos (pacman -Sp).
#  2. The root helper installs a module (ai-sessions: the smallest), and pacman, `wol modules status`
#     and `wol modules enable` (as the person, no password) agree on its state.
#  3. Queue and drain (installer offline -> firstboot): `modules-install --queue` writes
#     /var/lib/wolos/modules-pending, `--drain` installs it and empties the file.
#  4. ai-local's GPU backend follows `wol hw compute` (a VM has no GPU: cpu, no ollama-* backend).
#  5. The files the modules rely on are in place: Claude Code's managed settings, the WolOS skill,
#     zellij, mise and the console setup (a login zsh of the person has the WolOS history size), and
#     `wol sessions list --json` answers.
#  6. Terminals open the shell chosen in Settings (/usr/bin/wol-term-shell, zsh without a choice),
#     and with the console module installed, the person's pwsh (wopkg recipe) has the WolOS setup,
#     its launcher entry and the system shim; prints pwsh's start time with and without the setup.
# Run as root with the desktop user as argument: sudo tests/test-modules.sh alice
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
for id in ai-claude ai-local ai-sessions dev creative gaming console; do
  echo "$J" | grep -q "\"id\": *\"$id\"" || fail "wol modules list lacks $id"
  pacman -Sp --print-format %n "wolos-module-$id" >/dev/null || fail "wolos-module-$id does not resolve in the repos"
done

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
for f in /etc/claude-code/managed-settings.json /usr/share/wolos/agents/skills/wolos/SKILL.md \
  /etc/zellij/config.kdl /etc/mise/config.toml /usr/share/wolos/starship/starship.toml \
  /etc/profile.d/wolos-console.sh /etc/fish/conf.d/wolos.fish; do
  [ -r "$f" ] || fail "$f missing"
done
grep -q '"mcp__wolos"' /etc/claude-code/managed-settings.json || fail "managed settings do not allow wol mcp"
H=$(as_user zsh -lic 'print -r -- $HISTSIZE' 2>/dev/null | tail -1)
[ "$H" = 50000 ] || fail "a login zsh of $U has HISTSIZE=$H, not the WolOS 50000"
as_user wol sessions list --json | grep -q '"windows"' || fail "wol sessions list --json"

# 6. Terminal shell and pwsh.
TS=/usr/bin/wol-term-shell
[ -x "$TS" ] || fail "$TS missing"
[ -x /usr/lib/wolos/pwsh ] || fail "/usr/lib/wolos/pwsh missing"
Z=$(as_user env -u XDG_CONFIG_HOME "$TS" -fc 'print -r -- zsh:$ZSH_VERSION' 2>/dev/null | tail -1)
case $Z in zsh:?*) ;; *) fail "wol-term-shell without a choice is not zsh: $Z" ;; esac
if pacman -Qq wolos-module-console >/dev/null 2>&1; then
  as_user wol modules enable console </dev/null || fail "wol modules enable console"
  UH=$(getent passwd "$U" | cut -d: -f6)
  PW=$UH/.local/bin/pwsh
  [ -x "$PW" ] || fail "$PW missing after enabling console"
  [ -r "$UH/.local/share/applications/wolos-pwsh.desktop" ] || fail "pwsh launcher entry missing"
  N=$(as_user env WOLOS_PWSH_FORCE=1 "$PW" -NoLogo -c '(Get-PSReadLineOption).MaximumHistoryCount' 2>/dev/null | tail -1)
  [ "$N" = 50000 ] || fail "pwsh setup not loaded: MaximumHistoryCount=$N"
  as_user /usr/lib/wolos/pwsh -NoProfile -c 'exit 7' && fail "shim lost the exit code"
  [ $? = 7 ] || fail "shim exit code"
  t() { local s e; s=$(date +%s%N); for _ in 1 2 3 4 5; do as_user "$@" >/dev/null 2>&1; done; e=$(date +%s%N); echo $(( (e - s) / 5000000 )); }
  echo "test-modules: pwsh start ms: bare $(t "$PW" -NoLogo -NoProfile -c exit), setup $(t env WOLOS_PWSH_FORCE=1 "$PW" -NoLogo -c exit)"
else
  echo "test-modules: console module not installed: pwsh checks skipped"
fi

echo "test-modules: PASS"
