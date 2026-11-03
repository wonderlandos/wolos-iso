#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# test-audio.sh: inside an installed WolOS with a desktop user logged in, prove the sound rules of
# docs/modules/audio.md end to end (PipeWire 1.6 tools and metadata keys):
#  1. PipeWire loaded the WolOS fragment (default.clock.min-quantum 64, 96 kHz among the allowed
#     rates, per `pw-config merge`) and the effective floor (settings metadata clock.min-quantum) is
#     64 frames on hardware. In a VM (cpu.vm.name set) PipeWire's own rule raises it to 1024: the
#     graph never goes below that there, and the test expects exactly that.
#  2. A stream asking for 64 frames (`pw-cat --latency 64`, samples) on a null sink plays with
#     0 xruns and runs the graph at the floor (pw-top: driver QUANT = 64, or 1024 in a VM).
#  3. Pro Audio's forced buffer applies (driver QUANT 256, in a VM too) and reverts.
#  4. Echo cancel and the App Mix are pipewire-pulse modules: they load on demand and unload;
#     the echo canceller runs with WebRTC gain control on (aec.args webrtc.gain_control = true).
#  5. `wol audio reset` is a dry run (names wpctl clear-default, runs nothing) and
#     `wol audio doctor` passes.
# Needs the desktop user's session (its /run/user/<uid> and PipeWire); the live installer session
# has none. From root: runuser -u <user> -- env XDG_RUNTIME_DIR=/run/user/<uid> \
#   DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/<uid>/bus sh test-audio.sh
# Exit 0 and "test-audio: PASS", else the first failing step. Nothing it creates survives: cleanup
# on EXIT.
set -eu
fail() { echo "test-audio: FAIL: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 missing"; }

# 0. A desktop user session, not root, not the live installer.
[ "$(id -u)" != 0 ] || fail "no user session: run as the logged-in desktop user, not root (see the header)"
[ -n "${XDG_RUNTIME_DIR:-}" ] && [ -d "$XDG_RUNTIME_DIR" ] \
  || fail "no user session: XDG_RUNTIME_DIR is unset or missing (log in on the desktop first)"
ST=$(loginctl show-user "$(id -u)" -p State --value 2>/dev/null || :)
case $ST in active|online) ;; *) fail "no user session: loginctl reports user $(id -un) as '${ST:-absent}'" ;; esac
[ -S "$XDG_RUNTIME_DIR/pipewire-0" ] || fail "no user session PipeWire: $XDG_RUNTIME_DIR/pipewire-0 missing"
for t in pw-config pw-metadata pw-cat pw-top pw-dump pactl wpctl wol; do need "$t"; done

MODS= P=
cleanup() {
  [ -z "$P" ] || kill "$P" 2>/dev/null || :
  for m in $MODS; do pactl unload-module "$m" 2>/dev/null || :; done
  pw-metadata -n settings 0 clock.force-quantum 0 >/dev/null 2>&1 || :
  wol audio appmix off >/dev/null 2>&1 || :
}
trap cleanup EXIT

# graph NODE: "QUANT ERR" of NODE in the last pw-top sample (the first sample has no timing yet).
# Columns: S ID QUANT RATE WAIT BUSY W/Q B/Q ERR FORMAT NAME; a follower's NAME has a "+"/"=" prefix.
graph() {
  pw-top -b -n 3 2>/dev/null | awk -v n="$1" '
    $1 == "S" && $2 == "ID" { q = ""; e = "" }
    $NF == n && $1 == "R" { q = $3; e = $9 }
    END { print q, e }'
}
# play SECONDS [pw-cat args]: silence-like noise into the test sink, in the background ($P).
play() {
  s=$1; shift
  head -c $((48000 * 4 * s)) /dev/urandom | pw-cat --playback --raw --rate 48000 --channels 2 \
    --format s16 --target wolos_test -P '{ node.name = wolos_test_play }' "$@" - &
  P=$!
}

# 1. The fragment is what PipeWire loaded; the floor in effect is the fragment's (or the VM's).
C=$(pw-config -n pipewire.conf merge context.properties 2>/dev/null) || fail "pw-config merge context.properties"
echo "$C" | grep -q '"default.clock.min-quantum": *64\b' \
  || fail "the WolOS fragment's default.clock.min-quantum = 64 is not in the merged config: $C"
S=$(pw-metadata -n settings 0 2>/dev/null) || fail "pw-metadata: is pipewire running for this user?"
VM=$(pw-dump 2>/dev/null | grep -m1 -o '"cpu.vm.name": *"[^"]*"' | sed 's/.*: *"//; s/"$//' || :)
if [ -n "$VM" ]; then FLOOR=1024; else FLOOR=64; fi
echo "test-audio: cpu.vm.name='${VM}' -> expected floor $FLOOR frames"
echo "$S" | grep -q "key:'clock.min-quantum' value:'$FLOOR'" || fail "clock.min-quantum is not $FLOOR: $S"
echo "$S" | grep "key:'clock.allowed-rates'" | grep -q 96000 || fail "96000 not among clock.allowed-rates: $S"
systemctl --user is-active wireplumber.service >/dev/null || fail "wireplumber not active"

# 2. A 64-frame stream on a null sink: the graph runs at the floor, 0 xruns.
MODS="$(pactl load-module module-null-sink sink_name=wolos_test sink_properties=device.description=wolos_test) $MODS"
sleep 0.5
play 5 --latency 64
sleep 0.5
set -- $(graph wolos_test); GQ=${1:-} GE=${2:-}
set -- $(graph wolos_test_play); SQ=${1:-} SE=${2:-}
wait "$P" || fail "pw-cat could not play with --latency 64"; P=
echo "test-audio: --latency 64 stream: asks $SQ, graph quantum $GQ, xruns driver $GE stream $SE"
[ "$SQ" = 64 ] || fail "the stream did not ask for 64 frames (pw-top QUANT '$SQ')"
[ "$GQ" = "$FLOOR" ] || fail "graph quantum under a 64-frame stream is '$GQ', not the floor $FLOOR"
[ "$GE" = 0 ] && [ "$SE" = 0 ] || fail "xruns at the floor: driver '$GE', stream '$SE'"

# 3. Forced buffer applies and reverts.
pw-metadata -n settings 0 clock.force-quantum 256 >/dev/null
play 4
sleep 0.5
set -- $(graph wolos_test); GQ=${1:-}
wait "$P" || :; P=
echo "test-audio: clock.force-quantum 256: graph quantum $GQ"
[ "$GQ" = 256 ] || fail "forced quantum 256 not applied (graph quantum '$GQ')"
pw-metadata -n settings 0 clock.force-quantum 0 >/dev/null
pw-metadata -n settings 0 | grep -q "key:'clock.force-quantum' value:'0'" || fail "force-quantum did not revert"

# 4. On-demand modules: echo cancel (the "Automatic input level" piece) and the App Mix.
# The arguments are wol_core::audio::echo_cancel_argv's (crates/core/src/audio.rs): pactl joins
# them with spaces, so aec_args is quoted inside its argument, and the WebRTC keys are PipeWire's.
AEC='webrtc.gain_control=true webrtc.noise_suppression=true webrtc.voice_detection=true'
AEC="$AEC webrtc.high_pass_filter=true webrtc.extended_filter=true"
M=$(pactl load-module module-echo-cancel source_name=wolos_test_agc \
  source_master=wolos_test.monitor sink_master=wolos_test aec_method=webrtc \
  "aec_args=\"$AEC\"" "source_properties='device.description=\"WolOS Microphone\"'") \
  || fail "module-echo-cancel did not load"
MODS="$M $MODS"
sleep 0.5
# Loaded is not enough (A2): the AEC must run with gain control on. PipeWire 1.6's pipewire-pulse
# runs libpipewire-module-echo-cancel inside its own process and no longer exports that module (or
# its aec.args) to pw-dump, and the WebRTC engine publishes no gain-control param. So: pw-dump
# proves the canceller's nodes run (one echo-cancel node.group with the source in it), and
# pipewire-pulse's own record of the module (pactl list modules) proves aec_args, which it hands
# on as aec.args, carries webrtc.gain_control=true.
DUMP=$(pw-dump 2>/dev/null | tr -d '\\\n') || fail "pw-dump"
echo "$DUMP" | grep -q '"node.name": *"wolos_test_agc"' || fail "no node named wolos_test_agc"
echo "$DUMP" | grep -q '"node.group": *"echo-cancel-' || fail "no echo-cancel node group in pw-dump"
pactl list modules | awk -v m="Module #$M" '$0 == m {on = 1; next} /^Module #/ {on = 0} on' \
  | grep '^[[:space:]]*Argument:' | grep -q 'aec_args="[^"]*webrtc.gain_control=true' \
  || fail "echo-cancel module $M runs without webrtc.gain_control=true in aec_args"
pactl list short sources | grep -q wolos_test_agc || fail "echo-cancel source missing"
pactl unload-module "$M"; MODS=$(echo "$MODS" | sed "s/^$M //")
pactl list short sources | grep -q wolos_test_agc && fail "echo-cancel source survived unload"
wol audio appmix on >/dev/null || fail "wol audio appmix on"
pactl list short sinks | grep -q wolos_appmix || fail "WolOS App Mix sink missing"
pactl list short sources | grep -q wolos_mic_mix || fail "Mic + App Mix source missing"
wol audio appmix off >/dev/null || fail "wol audio appmix off"
pactl list short sinks | grep -q wolos_appmix && fail "App Mix survived off"

# 5. Reset is a dry run; the doctor passes. Piped, wol prints JSON: the plan's commands are argv
# arrays, so compare with the whitespace gone.
T0=$(systemctl --user show pipewire.service -p ActiveEnterTimestampMonotonic --value)
R=$(wol audio reset) || fail "wol audio reset (dry run) failed"
F=$(echo "$R" | tr -d ' \n')
echo "$F" | grep -q '"dry_run":true' || fail "wol audio reset without --yes is not a dry run: $R"
echo "$F" | grep -q '\["wpctl","clear-default"\]' || fail "reset dry run does not name wpctl clear-default: $R"
systemctl --user is-active pipewire.service >/dev/null || fail "the dry run stopped pipewire"
[ "$(systemctl --user show pipewire.service -p ActiveEnterTimestampMonotonic --value)" = "$T0" ] \
  || fail "the dry run restarted pipewire"
wol audio doctor >/dev/null || fail "wol audio doctor: $(wol audio doctor 2>&1 || :)"
echo "test-audio: PASS"
