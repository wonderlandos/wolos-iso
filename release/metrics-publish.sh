#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# metrics-publish.sh RUN SITE: measure a release run and hand the numbers to the site.
# Why: the Proof and Compare pages print data/metrics.json, and every number in it must come from a
# real run through tests/metrics.sh (NS2), never typed in by hand. How: runs tests/metrics.sh on RUN
# (a release run as wolos-build lays it out, /scratch/rel012), which validates the result against
# release/metrics.schema.json, and only then writes SITE/data/metrics.json (SITE: a checkout of the
# site; its data/ folder must exist). A run whose numbers do not validate leaves the site's file as it was.
# Prints one line: which numbers the run filled and which stayed null.
set -euo pipefail
if [ $# -ne 2 ]; then
  echo "usage: release/metrics-publish.sh RUN SITE" >&2
  exit 2
fi
run=$1 site=$2
here=$(cd "$(dirname "$0")/.." && pwd)
[ -d "$run" ] || { echo "metrics-publish: $run is not a directory" >&2; exit 2; }
[ -d "$site/data" ] || { echo "metrics-publish: $site/data is not a directory (SITE: a checkout of the site)" >&2; exit 2; }
tmp=$(mktemp "$site/data/.metrics.json.XXXXXX")
trap 'rm -f "$tmp"' EXIT
bash "$here/tests/metrics.sh" "$run" "$tmp" || { echo "metrics-publish: the numbers of $run do not validate; $site/data/metrics.json left as it was" >&2; exit 1; }
chmod 644 "$tmp"
mv -f "$tmp" "$site/data/metrics.json"
python3 -I - "$site/data/metrics.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))
keys = ["boot_s", "install_s", "pages", "idle_cpu_ms_s", "shell_map_ms"]
have = [k for k in keys if d.get(k) is not None]
null = [k for k in keys if d.get(k) is None]
print("metrics-publish: %s -> data/metrics.json; measured: %s; null: %s" % (d["run"]["id"], " ".join(have) or "-", " ".join(null) or "-"))
PY
