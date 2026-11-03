#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Build the base VM of tests/e2e.sh from the kept install of a release run, without touching it:
#   tests/wolvm/e2e/make-base.sh [SRC_HOME [SRC_VM]]
# SRC_HOME  the wolvm home of the kept install (default /scratch/rel012/vm/inst-default/test/wolvm; read only)
# SRC_VM    its VM name (default install; it was made by tests/test-install.sh with an autoinstall)
# E2E_HOME  where the base lives (default /scratch/e2e-qa/base/wolvm), E2E_BASE its name (e2eqa-base)
# The kept install has no desktop user that can start a session: an autoinstall makes none, and the
# hand-made `tester` of the release run was not in group `seat`, so the compositor died in a restart loop
# (black screen). This script gives `tester` the group and the password "tester", boots, unlocks the
# session and saves the whole-VM snapshot "desktop" that every journey with `from: desktop` restores.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
W=$here/../wolvm
SRC_HOME=${1:-/scratch/rel012/vm/inst-default/test/wolvm} SRC=${2:-install}
export WOLVM_HOME=${E2E_HOME:-/scratch/e2e-qa/base/wolvm}
B=${E2E_BASE:-e2eqa-base}
mkdir -p "$WOLVM_HOME"
"$W" stop "$B" --force >/dev/null 2>&1 || true
"$W" clone "$SRC" "$B" --from-home "$SRC_HOME" --force
# the kept install had a bystander raw disk (extra0) for the installer test; savevm needs qcow2 everywhere
python3 -I - "$WOLVM_HOME/vms/$B/vm.json" <<'PY'
import json, sys
p = sys.argv[1]
c = json.load(open(p))
c["drives"] = []
json.dump(c, open(p, "w"), indent=1)
PY
"$W" start "$B"
"$W" wait-agent "$B" --timeout 180
"$W" ssh "$B" 'id tester >/dev/null && usermod -aG seat tester && echo tester:tester | chpasswd && rm -rf /home/tester/.cache/hyprland'
"$W" ssh "$B" 'setsid -f sh -c "sleep 1; systemctl reboot" >/dev/null 2>&1 </dev/null; echo rebooting'
sleep 10
"$W" wait-agent "$B" --timeout 180
"$W" wait-text "$B" "login:" --timeout 5 --new >/dev/null 2>&1 || true
for _ in $(seq 60); do "$W" ssh "$B" 'pgrep -x wol-lock' >/dev/null 2>&1 && break; sleep 1; done
sleep 8
"$W" type "$B" 'tester\n' -e
for _ in $(seq 60); do "$W" ssh "$B" 'pgrep -x wol-lock' >/dev/null 2>&1 || break; sleep 1; done
for _ in $(seq 60); do "$W" ssh "$B" --user tester 'hyprctl layers -j | grep -q wol-dock' >/dev/null 2>&1 && break; sleep 1; done
sleep 5
"$W" click "$B" 640 300      # park the pointer on the wallpaper: no stray tooltip in the snapshot
sleep 2
"$W" snapshot "$B" save desktop
"$W" stop "$B"
echo "base $B ready in $WOLVM_HOME (snapshot desktop)"
