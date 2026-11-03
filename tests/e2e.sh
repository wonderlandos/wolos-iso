#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-or-later
# End-to-end journeys of every shipped app and flow, in VMs, on COPIES of one kept install.
#
#   tests/e2e.sh [-r RUNS] [-j PARALLEL] [-k] [-o OUTDIR] [JOURNEY...]
#
# JOURNEY is a name (login-unlock) or a file under tests/wolvm/journeys/e2e-*.yaml; none = all of them.
# Each run: clone the base VM (reflink, instant), start it, `snapshot restore desktop` (the journey's
# `from: desktop`) or cold boot (`from: cold`), replay the journey with a screenshot after every step,
# stop and delete the clone. RUNS > 1 runs every journey again from a fresh copy: a journey that
# passes in some runs and fails in others is reported as FLAKE, not FAIL.
# Env: E2E_HOME (wolvm home holding the base VM, default /scratch/e2e-qa/base/wolvm), E2E_BASE (VM name,
# default e2eqa-base; it needs the snapshot "desktop": tests/wolvm/e2e/make-base.sh), E2E_OUT.
# Output: OUTDIR/index.html (table + links), summary.md, summary.json, OUTDIR/<journey>/run<N>/ (report.md,
# index.html, PNGs, saved command output, serial.log).
set -u
here=$(cd "$(dirname "$0")" && pwd)
WOLVM=${WOLVM:-$here/wolvm/wolvm}
export WOLVM_HOME=${E2E_HOME:-/scratch/e2e-qa/base/wolvm}
BASE=${E2E_BASE:-e2eqa-base}
RUNS=1 PAR=3 KEEP=0 OUT=${E2E_OUT:-}
while getopts "r:j:ko:" o; do
  case $o in r) RUNS=$OPTARG ;; j) PAR=$OPTARG ;; k) KEEP=1 ;; o) OUT=$OPTARG ;; *) sed -n '3,9p' "$0"; exit 2 ;; esac
done
shift $((OPTIND - 1))
[ -n "$OUT" ] || OUT=/scratch/e2e-qa/out/$(date +%Y%m%d-%H%M%S)
mkdir -p "$OUT"

journeys=()
if [ $# -eq 0 ]; then
  for f in "$here"/wolvm/journeys/e2e-*.yaml; do journeys+=("$f"); done
else
  for j in "$@"; do
    f=$j; [ -f "$f" ] || f=$here/wolvm/journeys/e2e-${j#e2e-}.yaml
    [ -f "$f" ] || { echo "e2e: no journey $j" >&2; exit 2; }
    journeys+=("$f")
  done
fi

# meta KEY FILE: a top-level "key: value" of a journey file
meta() { sed -n "s/^$1:[[:space:]]*//p" "$2" | head -1; }

run_one() { # run_one JOURNEYFILE RUN
  f=$1 n=$2
  name=$(basename "$f" .yaml); name=${name#e2e-}
  vm=e2e-$name-$n
  d=$OUT/$name/run$n; mkdir -p "$d"
  from=$(meta from "$f"); from=${from:-desktop}
  {
    echo "== $name run $n: $(date +%T) VM $vm from $from"
    "$WOLVM" stop "$vm" --force >/dev/null 2>&1
    "$WOLVM" clone "$BASE" "$vm" --force || { echo "clone failed"; exit 3; }
    if [ "$from" = desktop ]; then
      # shellcheck disable=SC2015
      "$WOLVM" start "$vm" && "$WOLVM" snapshot "$vm" restore desktop || { echo "start/restore failed"; exit 3; }
      "$WOLVM" wait-agent "$vm" --timeout 60 || true
      # a restored VM shows the picture of its snapshot; let the pointer and the first frame settle
      "$WOLVM" wait-stable "$vm" --timeout 20 >/dev/null 2>&1
      sed '/^- start$/d' "$f" > "$d/journey.yaml"
    else
      cp "$f" "$d/journey.yaml"
    fi
    "$WOLVM" journey "$vm" "$d/journey.yaml" --out "$d" --keep-going
    rc=$?
    echo "== $name run $n: rc=$rc $(date +%T)"
    cp "$WOLVM_HOME/vms/$vm/serial.log" "$d/serial.log" 2>/dev/null
    "$WOLVM" stop "$vm" --force >/dev/null 2>&1
    [ "$KEEP" = 1 ] || "$WOLVM" rm "$vm" >/dev/null 2>&1
    exit $rc
  } > "$d/runner.log" 2>&1
}
export -f run_one meta
export WOLVM OUT BASE KEEP WOLVM_HOME

# shellcheck disable=SC2016
for n in $(seq 1 "$RUNS"); do
  for f in "${journeys[@]}"; do echo "$f $n"; done
done | xargs -P "$PAR" -L1 bash -c 'run_one "$0" "$1"'

python3 "$here/wolvm/e2e-report.py" "$OUT" "$RUNS"
echo "e2e: $OUT/index.html"
# exit status: 0 only when every journey passed in every run (flakes count as not passed)
python3 -c 'import json,sys; sys.exit(0 if all(j["verdict"]=="PASS" for j in json.load(open(sys.argv[1]))["journeys"]) else 1)' "$OUT/summary.json"
