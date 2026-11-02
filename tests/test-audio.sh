#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# test-audio.sh: inside a booted WolOS (live ISO in tests/run.sh with its intel-hda device, or an
# install), prove the sound rules of docs/modules/audio.md end to end:
#  1. PipeWire runs with the WolOS fragments: 64-frame floor, 96 kHz among the allowed rates.
#  2. A 64-frame (1.3 ms) stream on a null sink plays 2 s with 0 xruns (pw-top ERR column).
#  3. Pro Audio's forced buffer applies (pw-top QUANT 256) and reverts.
#  4. Echo cancel and the App Mix are pipewire-pulse modules: they load on demand and unload;
#     the echo canceller runs with WebRTC gain control on (aec.args webrtc.gain_control = true).
#  5. `wol audio reset` is a dry run (names wpctl clear-default, runs nothing) and
#     `wol audio doctor` passes.
# Run as the desktop user (needs the user's PipeWire). Exit 0 and "test-audio: PASS", else the
# first failing step. Nothing it creates survives: cleanup on EXIT.
set -eu
fail() { echo "test-audio: FAIL: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 missing"; }
for t in pw-metadata pw-cat pw-top pw-dump pactl wpctl wol; do need "$t"; done
MODS=
cleanup() {
  for m in $MODS; do pactl unload-module "$m" 2>/dev/null || :; done
  pw-metadata -n settings 0 clock.force-quantum 0 >/dev/null 2>&1 || :
  wol audio appmix off >/dev/null 2>&1 || :
}
trap cleanup EXIT

# 1. The fragments are what PipeWire loaded.
S=$(pw-metadata -n settings 0 2>/dev/null) || fail "pw-metadata: is pipewire running for this user?"
echo "$S" | grep -q "key:'clock.min-quantum' value:'64'" || fail "clock.min-quantum is not 64: $S"
echo "$S" | grep "key:'clock.allowed-rates'" | grep -q 96000 || fail "96000 not among clock.allowed-rates"
systemctl --user is-active wireplumber.service >/dev/null || fail "wireplumber not active"

# 2. 64-frame stream on a null sink, 0 xruns.
MODS="$(pactl load-module module-null-sink sink_name=wolos_test sink_properties=device.description=wolos_test) $MODS"
sleep 0.5
head -c 384000 /dev/urandom | pw-cat --playback --raw --rate 48000 --channels 2 --format s16 \
  --target wolos_test --latency 64/48000 - &
P=$!
TOP=$(pw-top -b -n 3 2>/dev/null)
wait $P || fail "pw-cat could not play at 64/48000"
ERR=$(echo "$TOP" | awk '$1!="S" && $NF ~ /wolos_test|pw-cat/ {sum+=$9} END {print sum+0}')
[ "$ERR" = 0 ] || fail "$ERR xrun(s) at 64 frames: $TOP"

# 3. Forced buffer applies and reverts.
pw-metadata -n settings 0 clock.force-quantum 256 >/dev/null
head -c 192000 /dev/urandom | pw-cat --playback --raw --rate 48000 --channels 2 --format s16 --target wolos_test - &
P=$!
sleep 0.5
pw-top -b -n 1 | awk '$1!="S" && $NF ~ /wolos_test/ {print $3}' | grep -qx 256 || fail "forced quantum 256 not applied"
wait $P || :
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
# Loaded is not enough (A2): the AEC must have been created with gain control on. pipewire-pulse
# hands aec_args to libpipewire-module-echo-cancel as aec.args; pw-dump shows that module's args
# (multi-line JSON, or one escaped string on older PipeWire: newlines and backslashes go).
DUMP=$(pw-dump 2>/dev/null | tr -d '\\\n') || fail "pw-dump"
echo "$DUMP" | grep -q '"node.name": *"wolos_test_agc"' || fail "no node named wolos_test_agc"
echo "$DUMP" | grep -o '"aec.args": *{[^}]*}' | grep -q '"webrtc.gain_control": *"\{0,1\}true' \
  || fail "echo-cancel runs without webrtc.gain_control = true"
pactl list short sources | grep -q wolos_test_agc || fail "echo-cancel source missing"
pactl unload-module "$M"; MODS=$(echo "$MODS" | sed "s/^$M //")
pactl list short sources | grep -q wolos_test_agc && fail "echo-cancel source survived unload"
wol audio appmix on >/dev/null || fail "wol audio appmix on"
pactl list short sinks | grep -q wolos_appmix || fail "WolOS App Mix sink missing"
pactl list short sources | grep -q wolos_mic_mix || fail "Mic + App Mix source missing"
wol audio appmix off >/dev/null || fail "wol audio appmix off"
pactl list short sinks | grep -q wolos_appmix && fail "App Mix survived off"

# 5. Reset is a dry run; the doctor passes.
R=$(wol audio reset) || fail "wol audio reset (dry run) failed"
echo "$R" | grep -q "wpctl clear-default" || fail "reset dry run does not name wpctl clear-default: $R"
systemctl --user is-active pipewire.service >/dev/null || fail "the dry run restarted pipewire"
wol audio doctor >/dev/null || fail "wol audio doctor: $(wol audio doctor 2>&1 || :)"
echo "test-audio: PASS"
