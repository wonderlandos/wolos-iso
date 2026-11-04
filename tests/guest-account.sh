#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# guest-account.sh: inside an installed WolOS, make the desktop account that test-audio.sh and
# test-modules.sh run as. The plain unattended install (wolos.autoinstall, tests/test-install.sh)
# creates no account: the Setup Assistant does on the first boot, which no test drives. So the
# release's guest tests add one here: a home, the groups a desktop person has (wheel, audio,
# video), and lingering, so systemd starts the account's user manager (/run/user/<uid>, PipeWire)
# without a login on the screen.
# Run as root: sh guest-account.sh [USER]   (default tester). Running it again changes nothing.
# Exit 0 and "guest-account: <user> uid <uid> ready", else the first failing step.
set -eu
fail() { echo "guest-account: FAIL: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
U=${1:-tester}
case $U in '' | *[!a-z0-9_-]* | [!a-z_]*) fail "'$U' is not an account name (a-z, 0-9, _ and -)" ;; esac
if ! id "$U" >/dev/null 2>&1; then
  useradd -m -G wheel,audio,video -s /usr/bin/zsh "$U" || fail "useradd $U"
fi
loginctl enable-linger "$U" || fail "loginctl enable-linger $U"
uid=$(id -u "$U")
t=0
until [ -d "/run/user/$uid" ] && systemctl is-active -q "user@$uid.service"; do
  t=$((t + 1)); [ "$t" -le 60 ] || fail "the user manager of $U (user@$uid.service) did not start in 60 s"
  sleep 1
done
echo "guest-account: $U uid $uid ready"
