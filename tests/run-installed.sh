#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Boot the installed disk ($WOLOS_VM/test/disk.raw from WOLOS_KEEP_DISK=1 tests/test-install.sh) in
# QEMU/KVM with OVMF and virtio-vga-gl. Serial console in $T/serial-installed.log.
# Env: WOLOS_VM (default ~/WolOS-vm), WOLOS_MEM (6G), WOLOS_DISPLAY (gtk,gl=on; egl-headless for CI).
#
# Group qa100: the QA-100 journeys gate against a wolvm VM (manifest tests/wolvm/qa100.manifest):
#   run-installed.sh qa100 --list                      every journey, its needs, its expected result
#   run-installed.sh qa100 --dry-run                   parse every journey (wolvm --dry-run journey)
#   run-installed.sh qa100 --vm NAME --have NEEDS [--restore TAG] [--only GLOB] [--out DIR] [--strict]
# NEEDS is a comma list of what this VM offers (installed,account:alice,network,displays:2,audio,s3,
# usb,disk-edit,tpm,...; account:any is met by any account:NAME). A journey whose needs are not all
# met is SKIP. The manifest's expect is pass, fail:<id>, fail:<id>@<steps> (the step numbers, comma
# list, where the bug shows: the journey must fail first at one of them) or fail:<id>@visual (no step
# asserts the bug; screenshots and saved output are the evidence). Each run journey ends
#   PASS   passed, expected to pass          FAIL   failed, expected to pass
#   XFAIL  failed as expected (at a pinned step, when the manifest pins one)
#   INFRA  expected to fail at a pinned step, but the first failure is another step (named): setup,
#          harness or VM state, not the bug
#   XPASS  passed although a failure is expected (fixed? check, then set pass)
#   REVIEW an @visual journey ran (passed or failed): a person judges its report
# --restore TAG restores wolvm snapshot TAG before each journey. Exit 1 on any FAIL (with --strict
# also on any XPASS or INFRA). Summary: DIR/qa100-summary.tsv. Env: WOLVM_HOME as for wolvm;
# WOLOS_WOLVM (another wolvm) and QA100_MANIFEST (another manifest) for the gate's own tests.
set -eu

qa100() {
  here=$(cd "$(dirname "$0")" && pwd)
  manifest=${QA100_MANIFEST:-$here/wolvm/qa100.manifest}
  wolvm=${WOLOS_WOLVM:-"python3 $here/wolvm/wolvm"}
  vm="" have="" restore="" only="*" out="" mode=run strict=0
  while [ $# -gt 0 ]; do
    case $1 in
      --list) mode=list ;;
      --dry-run) mode=dry ;;
      --vm) vm=${2:?--vm NAME}; shift ;;
      --have) have=${2:?--have NEEDS}; shift ;;
      --restore) restore=${2:?--restore TAG}; shift ;;
      --only) only=${2:?--only GLOB}; shift ;;
      --out) out=${2:?--out DIR}; shift ;;
      --strict) strict=1 ;;
      *) echo "run-installed.sh qa100: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
  done
  [ -f "$manifest" ] || { echo "run-installed.sh: no $manifest" >&2; exit 2; }
  if [ "$mode" = run ]; then
    [ -n "$vm" ] || { echo "run-installed.sh qa100: --vm NAME (a wolvm VM), or --list / --dry-run" >&2; exit 2; }
    $wolvm status "$vm" >/dev/null 2>&1 || { echo "run-installed.sh qa100: no wolvm VM $vm (WOLVM_HOME=${WOLVM_HOME:-default})" >&2; exit 2; }
    out=${out:-$PWD/qa100-$(date +%Y%m%d-%H%M%S)}
    mkdir -p "$out"
    printf 'journey\tresult\texpect\tneeds\tmissing\tfirst_failed_step\n' > "$out/qa100-summary.tsv"
  fi
  n=0 pass=0 xfail=0 fail=0 xpass=0 skip=0 infra=0 review=0 bad=0
  while IFS='|' read -r j needs expect why; do
    j=$(trim "$j")
    case $j in ''|'#'*) continue ;; esac
    # shellcheck disable=SC2254 # --only is a glob on purpose
    case $j in $only) ;; *) continue ;; esac
    needs=$(trim "$needs"); expect=$(trim "$expect"); why=$(trim "$why")
    n=$((n + 1))
    file=$here/wolvm/journeys/$j.yaml
    case $mode in
      list) printf '%-40s %-44s %s\n' "$j" "$needs" "$expect"; continue ;;
      dry)
        if [ ! -f "$file" ]; then echo "MISSING $j"; bad=$((bad + 1))
        elif steps=$($wolvm --dry-run journey qa100-dry "$file" 2>&1); then
          echo "ok      $j ($(printf '%s\n' "$steps" | grep -c '^[0-9]') steps)"
        else echo "BAD     $j: $(printf '%s\n' "$steps" | tail -n 1)"; bad=$((bad + 1)); fi
        continue ;;
    esac
    miss=$(missing "$needs" "$have") first=""
    case $expect in
      *@*) pin=${expect#*@} ;;
      *) pin="" ;;
    esac
    if [ -n "$miss" ]; then
      result=SKIP; skip=$((skip + 1))
    elif [ -n "$restore" ] && ! $wolvm snapshot "$vm" restore "$restore" >"$out/$j.restore.log" 2>&1; then
      result=FAIL; fail=$((fail + 1)); why="snapshot restore $restore failed ($out/$j.restore.log)"
    elif $wolvm journey "$vm" "$file" --keep-going --out "$out/$j" >"$out/$j.log" 2>&1; then
      if [ "$expect" = pass ]; then result=PASS; pass=$((pass + 1))
      elif [ "$pin" = visual ]; then result=REVIEW; review=$((review + 1)); first="(passed)"
      else result=XPASS; xpass=$((xpass + 1)); fi
    else
      first=$(first_failed "$out/$j/journey.jsonl")
      if [ "$expect" = pass ]; then result=FAIL; fail=$((fail + 1))
      elif [ "$pin" = visual ]; then result=REVIEW; review=$((review + 1))
      elif [ -z "$pin" ] || in_list "${first%% *}" "$pin"; then result=XFAIL; xfail=$((xfail + 1))
      else result=INFRA; infra=$((infra + 1)); fi
    fi
    note=${miss:+ (missing $miss)}
    [ "$result" = INFRA ] && note=" (first failure: step ${first:-none recorded}; expected at $pin)"
    printf '%-6s %-40s %s%s\n' "$result" "$j" "$expect" "$note"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$j" "$result" "$expect" "$needs" "$miss" "$first" >> "$out/qa100-summary.tsv"
  done < "$manifest"
  case $mode in
    list) echo "$n journeys"; summary_needs "$manifest"; return 0 ;;
    dry) echo "$n journeys, $bad not parsed"; [ "$bad" -eq 0 ]; return ;;
  esac
  echo "qa100: $n journeys: $pass PASS, $xfail XFAIL, $fail FAIL, $xpass XPASS, $review REVIEW, $skip SKIP; INFRA $infra ($out/qa100-summary.tsv)"
  [ "$fail" -eq 0 ] || return 1
  [ "$strict" -eq 0 ] || { [ "$xpass" -eq 0 ] && [ "$infra" -eq 0 ]; }
}

# first_failed JSONL: "N step words" of the first failed step of a journey run, or "" (no record:
# the journey did not even start, e.g. wolvm refused the VM).
first_failed() {
  [ -f "$1" ] || return 0
  python3 -c 'import json, sys
first = ""
for line in open(sys.argv[1]).read().splitlines():
    r = json.loads(line)
    if "journey" in r:
        first = ""
    elif not r.get("ok") and not first:
        first = "%d %s" % (r["n"], r["step"][:60])
print(first)' "$1"
}

# in_list N "3,7,9": is N one of them?
in_list() {
  case ",$2," in *",$1,"*) [ -n "$1" ] ;; *) return 1 ;; esac
}

trim() { printf '%s' "$1" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//'; }

# missing NEEDS HAVE: the needs HAVE does not cover (displays:N is met by displays:M, M >= N;
# account:any by any account:NAME).
missing() {
  m=""
  for need in $(printf '%s' "$1" | tr ',' ' '); do
    ok=0
    for h in $(printf '%s' "$2" | tr ',' ' '); do
      case $need:$h in
        "$h:$h") ok=1 ;;
        account:any:account:*) ok=1 ;;
        displays:*:displays:*) [ "${h#displays:}" -ge "${need#displays:}" ] 2>/dev/null && ok=1 ;;
      esac
    done
    [ $ok -eq 1 ] || m="${m:+$m,}$need"
  done
  printf '%s' "$m"
}

# The manifest counts: journeys per need and per expected result.
summary_needs() {
  grep -v '^[[:space:]]*#' "$1" | grep '|' | awk -F'|' '
    { gsub(/ /, "", $2); gsub(/ /, "", $3); n = split($2, a, ","); for (i = 1; i <= n; i++) need[a[i]]++
      if ($3 == "pass") p++; else f++ }
    END { printf "expect: %d pass, %d fail\nneeds:", p, f
          for (k in need) printf " %s=%d", k, need[k]; printf "\n" }'
}

if [ "${1:-}" = qa100 ]; then
  shift
  qa100 "$@"
  exit
fi

VM=${WOLOS_VM:-$HOME/WolOS-vm}; T=$VM/test
OVMF=${WOLOS_OVMF:-/usr/share/edk2/x64}
command -v qemu-system-x86_64 >/dev/null || { echo "$0: qemu-system-x86_64 missing (pacman -S qemu-desktop)" >&2; exit 1; }
[ -f "$OVMF/OVMF_CODE.4m.fd" ] || { echo "$0: no OVMF in $OVMF (pacman -S edk2-ovmf)" >&2; exit 1; }
mkdir -p "$T"
[ -f "$T/disk.raw" ] || { echo "run-installed.sh: no $T/disk.raw (WOLOS_KEEP_DISK=1 tests/test-install.sh)" >&2; exit 1; }
[ -f "$T/vars.fd" ] || cp "$OVMF/OVMF_VARS.4m.fd" "$T/vars.fd"
echo $$ > "$T/vm.pid"
exec qemu-system-x86_64 -enable-kvm -cpu host -smp "$(nproc)" -m "${WOLOS_MEM:-6G}" -machine q35 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF/OVMF_CODE.4m.fd" -drive if=pflash,format=raw,file="$T/vars.fd" \
  -drive file="$T/disk.raw",if=virtio,format=raw,discard=unmap \
  -device virtio-vga-gl -display "${WOLOS_DISPLAY:-gtk,gl=on}" \
  -nic user,model=virtio-net-pci -device qemu-xhci -device usb-tablet \
  -serial file:"$T/serial-installed.log" -qmp unix:"$T/qmp.sock",server,nowait
