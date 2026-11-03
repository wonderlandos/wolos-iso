# wolvm: drive a WolOS VM step by step (agents and people)

`tests/wolvm/wolvm` is one CLI over QEMU/KVM that boots an ISO, takes screenshots, types and
clicks, waits for text on the serial console, runs commands inside the guest, saves and restores
whole-VM snapshots, replays a journey (a list of steps with a screenshot after each) and writes an
HTML/markdown report. Python 3 stdlib only. It runs where QEMU runs (the forge); from Windows,
call it through `forge.sh run` and copy screenshots back with `scp`.

Design, the reasons behind it, and the full journey from boot to apps: `wolos/docs/modules/vm-e2e.md`.

## Check the host

```sh
tests/wolvm/wolvm doctor        # KVM, qemu, qemu-img, OVMF (+secboot), swtpm, virt-fw-vars, bsdtar, ssh
```

`MISS` lines are required, `warn` lines only matter for one option (`--tpm` needs swtpm,
`--secureboot` needs virt-fw-vars, `--direct` needs bsdtar). VMs live in `$WOLVM_HOME`
(default `~/.local/share/wolvm`); on the forge use the big disk: `export WOLVM_HOME=/scratch/<you>/wolvm`.

## Commands

| Command | What it does |
|---|---|
| `new NAME --iso X [--disk 40G --mem 6G --smp 4 --tpm --secureboot --gpu virtio\|vmsvga\|std\|qxl\|virtio-gl --res 1280x800 --ssh --direct --append ARGS --cred NAME=VALUE --vnc ADDR --force --drive FILE[,ro\|,snapshot]... --no-net --iso-bus ide\|virtio --from-image DISK]` | qcow2 disk + qcow2 UEFI vars (so snapshots cover the firmware too) |
| `start NAME` / `stop NAME [--force]` | daemonized qemu (+ swtpm); stop = ACPI power button, then quit, then kill |
| `status NAME` / `list` / `rm NAME` | JSON status: running, guest agent up, boot device, screen size |
| `set NAME --boot disk\|cdrom / --iso X / --no-iso / --append ARGS / --net on\|off` | for the next start (after install: `--boot disk`) |
| `set NAME --secureboot on\|off` | VM made with `--secureboot`, stopped: flips `SecureBootEnable` in `vars.qcow2` (virt-fw-vars); keys stay enrolled |
| `eject NAME` | take the ISO out of the running VM |
| `screenshot NAME OUT.png` | QMP screendump as PNG, prints `OUT.png WxH`; nudges the pointer 1 px first (`--no-nudge`) |
| `wait-stable NAME [--timeout S]` | until two screens 2 s apart are identical (animations done, page loaded) |
| `type NAME TEXT [-e]` | US keyboard; `-e` reads `\n`, `\t` |
| `key NAME COMBO...` | `ret esc tab spc backspace up down f2 ctrl-alt-t super shift-tab ctrl-alt-f3` (QEMU qcodes; aliases enter, super, win, del) |
| `click NAME X Y [--button right] [--double]` | absolute pixels of the last screenshot (usb-tablet) |
| `serial NAME tail [-n N] [--all]` / `serial NAME send TEXT` | the serial console since the last start |
| `wait-text NAME TEXT [--timeout S --regex --new]` | exit 0 when TEXT is on the serial console, else 1 with the last 25 lines |
| `wait-agent NAME [--timeout S]` | until qemu-guest-agent answers (the ISO and the installed system run it) |
| `ssh NAME CMD...` (alias `exec`) `[--user U] [--via auto\|ssh\|agent]` | a root shell command in the guest: sshd if the VM has `--ssh`, else the guest agent; exit code passes through |
| `ssh-open NAME` | `--ssh` VMs: through the agent, start sshd and accept port 22 in nftables (`inet wolos`), this boot only |
| `logs NAME [-n N] [-u UNIT] [-o FILE]` | `journalctl -b` of the current boot; serial log when the guest is unreachable |
| `snapshot NAME save\|restore\|list\|delete TAG` | running: the whole VM incl. RAM and TPM (savevm, ~4 s); stopped: disk + vars only |
| `journey NAME FILE [--out DIR --from N --keep-going]` | replay steps, screenshot after each, `journey.jsonl`, report |
| `report DIR` | `DIR/report.md` and `DIR/index.html` from a journey dir (or any dir of PNGs) |

`new --drive FILE` (repeatable) adds raw virtio disks after the VM's own, in order (`/dev/vdb`,
`/dev/vdc`...); wolvm never converts them, so a test can compare their bytes after the run (`,ro`
read-only, `,snapshot` writes thrown away). A raw writable extra disk means `snapshot save` of a
running VM fails (savevm needs qcow2 everywhere). `new --no-net` gives no network card (offline
installs). `new --iso-bus virtio` attaches the ISO as a writable throwaway virtio disk
(`snapshot=on`), like a USB stick, instead of a CD drive; works with `--direct`.

`new --from-image DISK` starts from a copy of an existing raw/qcow2 disk (an installed system,
or a prepared ESP such as `tests/wolvm/mok-esp.sh` builds).

`--dry-run` before the command prints the qemu command line (`new`, `start`) or the QMP JSON
(`key`, `type`, `click`, `screenshot`, `stop`, `snapshot`) and changes nothing.

How the guest is reached without changing the ISO:
- **Serial console**: wolvm passes `console=tty0 console=ttyS0,115200` as SMBIOS type 11
  `io.systemd.boot.kernel-cmdline-extra` (and `io.systemd.stub.`), which systemd-boot appends to the
  live entry and to the installed system's entry. systemd ignores it with Secure Boot on: use
  `--direct` (boot the ISO's kernel with our own cmdline) or the guest agent there.
- **Guest agent**: a virtio-serial port `org.qemu.guest_agent.0`; `qemu-guest-agent` (in the ISO's
  package list) starts by itself. `guest-exec` gives a root shell command with output.
- **ssh** (`--ssh`): a key pair per VM, the public key as the systemd credential
  `ssh.authorized_keys.root` (SMBIOS), a host port forwarded to 22. Needs sshd running in the guest
  The image ships sshd disabled and an input policy of drop, so after `wait-agent` run
  `wolvm ssh-open NAME` (tested on v0.11.5: root over ssh, key from the credential in
  `/root/.ssh/authorized_keys`). Without it the forwarded port times out at the banner.
- **Kernel words**: `--append "wolos.try"`, `--append "wolos.autoinstall=/dev/vda"` (disk is virtio: `/dev/vda`). They reach only boots from the ISO; after `set --boot disk` / `boot disk` the installed system gets the console words alone.

## The agent loop

```sh
F=C:/Users/klee/Documents/wolos/.claude/bin/forge.sh
V='WOLVM_HOME=/scratch/<you>/wolvm wolos-iso/tests/wolvm/wolvm'
bash $F run "$V new live --iso /scratch/<you>/wolvm/iso/wolos.iso --tpm && $V start live"
bash $F run "$V wait-agent live --timeout 300 && $V wait-stable live && $V screenshot live /scratch/<you>/wolvm/now.png"
scp -i C:/Users/klee/Documents/wolos/.claude/keys/forge arch@$(bash $F ip):/scratch/<you>/wolvm/now.png C:/Users/klee/Documents/wolos/.claude/tmp/wolvm/
# Read the PNG (Read tool), decide, act, repeat:
bash $F run "$V snapshot live save before-disk-page; $V click live 824 599; $V wait-stable live; $V screenshot live /scratch/<you>/wolvm/now.png"
```

1. Look at the screenshot. Name what is on screen (page title, focused control, dialogs).
2. Pick the smallest action: `key tab`/`key ret` for focused buttons, `click X Y` on a button
   centre read from the image (coordinates are image pixels), `type` for text fields.
3. `wait-stable`, screenshot, check the action did what you expected. Not: `snapshot restore` and
   try another way instead of piling up on a broken state.
4. Ask the system, not only the pixels: `ssh NAME 'journalctl -b -n 50 -t wol-installer'`,
   `ssh NAME 'systemctl --failed'`, `logs NAME -u wolos-installer`.
5. Once a path works, write it into a journey file so the next run is one command.

## Journeys

YAML subset (or JSON): top-level `name:`, then `- verb args` or `- verb: value` with indented
options. Verbs: `start stop shot type key click wait-text wait-agent wait-stable sleep ssh exec logs
checkpoint snapshot-save snapshot-restore eject boot note serial-send`. Options for every step:
`timeout`, `retry`, `on-fail-restore: TAG`, `shot: false`, `caption`; `ssh`: `expect`, `rc` (number
or `any`), `save: file.txt`, `user`; `wait-text`: `regex`, `new`; `click`: `button`, `double`.

```yaml
name: live ISO boots to the installer
steps:
- start
- wait-text: "Welcome to"
  timeout: 240
- wait-agent:
  timeout: 300
- wait-stable:
  timeout: 120
- checkpoint: installer-open
- ssh: "systemctl --failed --no-legend"
  rc: any
  save: failed-units.txt
- logs:
  n: 500
```

Examples: `tests/wolvm/journeys/live-boot.yaml`, `install-auto.yaml` (wolos.autoinstall, reboot
into the disk), `install-gui.yaml` (a keyboard template the agent adapts screen by screen).
A failed step stops the run; fix the step and rerun with `--from N` after `snapshot restore`.
Each run writes `journey.jsonl`, the PNGs, saved command output, `serial.log`, `report.md`, `index.html`.

## Tests

```sh
python3 tests/wolvm/test_wolvm.py     # step parser, key/QMP builders, qemu command, PNG writer, report, dry runs
```

## Known traps

- Screens that look stale: the compositor on `virtio-vga` (no GL) only flushes damage; wolvm nudges the
  pointer before each screendump. `virtio-gl` screendumps can fail with "no surface": wolvm then
  fails instead of returning an old frame.
- Secure Boot (`--secureboot`): Microsoft + Red Hat keys enrolled with virt-fw-vars; the serial
  console extra is ignored, so rely on the guest agent. The v0.11.5 ISO is not signed
  (`EFI/BOOT/BOOTx64.EFI` is systemd-boot with an empty certificate table): OVMF refuses it,
  "Access Denied -- rejected probably by Secure Boot", "No bootable option or device was found".
- MokManager (the installer's MOK variant): `mok-esp.sh` puts Fedora's signed shim (wolos-shim)
  in front of an unenrolled loader; `journeys/mok-screen.yaml` walks the blue screens: ERROR
  "Verification failed: (0x1A) Security Violation" [OK] -> "Shim UEFI key management / Press any
  key to perform MOK management" (10 s countdown, then it continues booting) -> "Perform MOK
  management": Continue boot / Enroll key from disk / Enroll hash from disk -> "Select Key"
  (volume list). The firmware text is on the serial console, so `wait-text` finds each screen.
- `snapshot save` needs every writable disk in qcow2 (wolvm makes disk and vars qcow2).
