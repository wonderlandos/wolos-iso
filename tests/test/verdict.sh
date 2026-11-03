#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# verdict.sh PREFIX FILE...: the exit status of a VM test run from its result lines (audit5 I6-2).
# Each FILE must exist and hold at least one "PREFIX PASS" line and no "PREFIX FAIL" line; a FILE
# without any (a scenario that never got to its checks) counts as a failure. Prints one summary
# line; exit 0 when every FILE passed, else 1. Used by test-install.sh (BOOT) and
# test-installer.sh (E2E).
set -u
p=${1:?usage: verdict.sh PREFIX FILE...}; shift
[ $# -gt 0 ] || { echo "verdict: no result files" >&2; exit 1; }
pass=0 fail=0 bad=0
for f in "$@"; do
  if [ ! -f "$f" ]; then echo "verdict: $f: no results" >&2; bad=$((bad + 1)); continue; fi
  np=$(grep -c "^$p PASS" "$f"); nf=$(grep -c "^$p FAIL" "$f")
  pass=$((pass + np)) fail=$((fail + nf))
  if [ "$nf" -gt 0 ]; then bad=$((bad + 1))
  elif [ "$np" = 0 ]; then echo "verdict: $f: no $p PASS line (the checks never ran)" >&2; bad=$((bad + 1)); fi
done
echo "verdict: $pass passed, $fail failed, $bad of $# result files not clean"
[ "$bad" = 0 ]
