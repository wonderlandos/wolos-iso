#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# test-secureboot.sh ISO CERT.pem: the signed live ISO boots with Secure Boot on (ADR 0025).
# OVMF in secure mode with the usual PC keys (Microsoft's, which sign shim: virt-fw-vars
# --enroll-redhat) and the WolOS certificate already in MokList, as MokManager leaves it after
# "Enroll key from disk". Passes when the live system answers through the QEMU guest agent with
# Secure Boot enabled, started by shim -> systemd-boot -> EFI/Linux/wolos.efi.
# Also (audit3 R2B-9): the image's sbctl, as boot-setup runs it for own keys (`enroll-keys --microsoft`),
# puts Microsoft's 2023 certificates next to the 2011 ones. Asked inside the booted guest with
# `--export esl` (writes db.esl/KEK.esl to a temp folder, touches no firmware variable), the test
# greps their subjects: db needs "Windows UEFI CA 2023" (signs current Windows boot managers),
# "Microsoft UEFI CA 2023" (third-party: shims), "Microsoft Option ROM UEFI CA 2023" (add-in
# cards) and the 2011 pair; KEK needs "Microsoft Corporation KEK 2K CA 2023". Missing one: FAIL
# (an own-keys install would leave Windows or a GPU unable to start after Microsoft's switch).
# sbctl 0.18-2 checked on the forge 2026-11-02 (the same export, outside a VM): all present.
# Needs qemu, edk2-ovmf, virt-firmware, python3 and KVM. Without CERT (no WolOS Secure Boot key
# yet) it reports a skip and exits 0. Work dir: $WOLOS_SB_TEST (default /tmp/wolos-sb-test).
set -euo pipefail
ISO=${1:?ISO}; CERT=${2:-}
if [ -z "$CERT" ] || [ ! -s "$CERT" ]; then
  echo "test-secureboot: SKIP (no WolOS Secure Boot certificate yet)"; exit 0
fi
D=${WOLOS_SB_TEST:-/tmp/wolos-sb-test}; F=${WOLOS_OVMF:-/usr/share/edk2/x64}
SHIM_GUID=605dab50-e046-4300-abb6-3dd810dd8b23
rm -rf "$D"; mkdir -p "$D"
virt-fw-vars --input "$F/OVMF_VARS.4m.fd" --output "$D/vars.fd" --enroll-redhat --secure-boot \
  --add-mok "$SHIM_GUID" "$CERT" >/dev/null 2>&1
timeout 600 qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m 6G -machine q35,smm=on \
  -global driver=cfi.pflash01,property=secure,value=on \
  -drive if=pflash,format=raw,readonly=on,file="$F/OVMF_CODE.secboot.4m.fd" \
  -drive if=pflash,format=raw,file="$D/vars.fd" -cdrom "$ISO" -device virtio-vga -display none \
  -nic user,model=virtio-net-pci -chardev socket,path="$D/qga.sock",server=on,wait=off,id=qga0 \
  -device virtio-serial -device virtserialport,chardev=qga0,name=org.qemu.guest_agent.0 \
  -serial file:"$D/serial.log" > "$D/qemu.log" 2>&1 &
Q=$!
trap 'kill $Q 2>/dev/null || :' EXIT
# Inside the guest: the boot facts, then the Microsoft subjects sbctl --microsoft exports.
GUEST='mokutil --sb-state; bootctl status 2>/dev/null
w=$(mktemp -d) && cd "$w" && printf %s "$(cat /proc/sys/kernel/random/uuid)" > GUID &&
printf "keydir: %s/keys\nguid: %s/GUID\n" "$w" "$w" > sbctl.conf &&
sbctl --config "$w/sbctl.conf" --disable-landlock create-keys >/dev/null 2>&1 &&
sbctl --config "$w/sbctl.conf" --disable-landlock enroll-keys --microsoft --export esl >/dev/null 2>&1
for f in db KEK; do
  for cn in "Microsoft Corporation UEFI CA 2011" "Microsoft UEFI CA 2023" "Microsoft Option ROM UEFI CA 2023" \
    "Microsoft Windows Production PCA 2011" "Windows UEFI CA 2023" "Microsoft Corporation KEK 2K CA 2023"; do
    grep -aqF "$cn" "$w/$f.esl" 2>/dev/null && echo "sbctl-$f: $cn"
  done
done
rm -rf "${w:?}"'
out=$(python3 - "$D/qga.sock" "$GUEST" <<'PY'
import base64, json, socket, sys, time
deadline = time.time() + 420
while time.time() < deadline:
    try:
        s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.settimeout(5)
        f = s.makefile("rw")
        def cmd(c, **a):
            f.write(json.dumps({"execute": c, "arguments": a} if a else {"execute": c}) + "\n"); f.flush()
            return json.loads(f.readline())
        cmd("guest-sync", id=7)
        pid = cmd("guest-exec", path="/bin/sh", arg=["-c", sys.argv[2]],
                  **{"capture-output": True})["return"]["pid"]
        for _ in range(30):
            r = cmd("guest-exec-status", pid=pid)["return"]
            if r.get("exited"):
                print(base64.b64decode(r.get("out-data", "")).decode(errors="replace")); sys.exit(0)
            time.sleep(1)
    except (OSError, ValueError, KeyError):
        time.sleep(5)
print("no answer from the guest"); sys.exit(1)
PY
) || { echo "test-secureboot: FAIL: the ISO did not boot with Secure Boot on"; echo "$out"; exit 1; }
echo "$out" | grep -E "SecureBoot|Secure Boot|Loader:|Current Entry" || :
echo "$out" | grep -q "SecureBoot enabled" || { echo "test-secureboot: FAIL: Secure Boot not enabled"; exit 1; }
echo "$out" | grep -q "grubx64.efi" || { echo "test-secureboot: FAIL: not started by shim's second stage"; exit 1; }
echo "$out" | grep -q "Current Entry: wolos.efi" || { echo "test-secureboot: FAIL: not the UKI"; exit 1; }
for want in "sbctl-db: Microsoft Corporation UEFI CA 2011" "sbctl-db: Microsoft UEFI CA 2023" \
  "sbctl-db: Microsoft Option ROM UEFI CA 2023" "sbctl-db: Microsoft Windows Production PCA 2011" \
  "sbctl-db: Windows UEFI CA 2023" "sbctl-KEK: Microsoft Corporation KEK 2K CA 2023"; do
  echo "$out" | grep -qxF "$want" || { echo "test-secureboot: FAIL: own keys lack ${want#*: }"; exit 1; }
done
echo "test-secureboot: PASS"
