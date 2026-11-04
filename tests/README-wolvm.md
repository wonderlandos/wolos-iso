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
`--secureboot` needs virt-fw-vars, `--direct` needs bsdtar, `disk-edit` needs qemu-nbd and, for an
encrypted disk, cryptsetup). VMs live in `$WOLVM_HOME`
(default `~/.local/share/wolvm`); on the forge use the big disk: `export WOLVM_HOME=/scratch/<you>/wolvm`.

## Commands

| Command | What it does |
|---|---|
| `new NAME --iso X [--disk 40G --mem 6G --smp 4 --tpm --secureboot --gpu virtio\|vmsvga\|std\|qxl\|virtio-gl --res 1280x800 --ssh --direct --append ARGS --cred NAME=VALUE --vnc ADDR --force --drive FILE[,ro\|,snapshot]... --no-net --iso-bus ide\|virtio --from-image DISK]` | qcow2 disk + qcow2 UEFI vars (so snapshots cover the firmware too) |
| `start NAME` / `stop NAME [--force]` | daemonized qemu (+ swtpm); stop = ACPI power button, then quit, then kill |
| `status NAME` / `list` / `rm NAME` | JSON status: running, guest agent up, boot device, screen size |
| `set NAME --boot disk\|cdrom / --iso X / --no-iso / --append ARGS / --net on\|off` | for the next start (after install: `--boot disk`) |
| `new NAME ... --audio N --displays N --s3 --no-tpm --disk-bus virtio\|nvme\|sata --disk-lbs 512\|4096 --disk-pbs 512\|4096 --extra-bus B --extra-lbs B --extra-pbs B --qemu-arg ARGS...` | the hardware the QA-100 hunters needed (below) |
| `set NAME --audio N / --displays N / --s3 on\|off / --tpm on\|off` | the same on an existing VM, for the next start |
| `set NAME --secureboot on\|off` | VM made with `--secureboot`, stopped: flips `SecureBootEnable` in `vars.qcow2` (virt-fw-vars); keys stay enrolled |
| `eject NAME` | take the ISO out of the running VM |
| `screenshot NAME OUT.png [--head N]` | QMP screendump as PNG, prints `OUT.png WxH`; nudges the pointer 1 px first (`--no-nudge`); `--head N`: head N of a `--displays` VM (0 is the first) |
| `frames NAME DIR [--every MS --for S --prefix P --head N]` | burst of screendumps, no nudge: `DIR/frame-000.png`... one every MS (default 150) for S seconds (default 10), as fast as QEMU writes them if slower |
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
| `qmp NAME JSON\|COMMAND [ARGS-JSON]` | one raw QMP command (`qmp v system_wakeup`, `qmp v device_del '{"id":"x"}'`), prints the reply |
| `plug NAME usb-audio\|usb-storage\|usb-kbd\|usb-mouse [--file IMG --id ID --ro]` | hot-plug a USB device on the xhci bus; `usb-storage` = a raw image as a removable stick (`blockdev-add` + `device_add`), `usb-audio` plays into the null audiodev |
| `unplug NAME ID` | `device_del`, waits until the guest let go, then `blockdev-del` of a stick's image |
| `display NAME N on\|off` | plug or unplug connector `Virtual-N` in the guest (DRM debugfs `force` + udev change: the compositor sees a hotplug); `--displays` VMs |
| `suspend NAME` / `wake NAME [--timeout S]` | S3 (`--s3` VMs): `systemctl suspend` through the agent, until QEMU says `suspended`; `system_wakeup`, until `running` |
| `power-cut NAME --at REGEX [--timeout S --delay S]` | polls the live system's install log (`wolos-install:` lines, guest agent; serial console as fallback) until a stage matches, waits `--delay`, kills QEMU (nothing synced) |
| `disk-edit NAME [--nbd /dev/nbdN --key-file F --read-only --no-snapshot] CMD` | stopped VM: the installed disk on the host through qemu-nbd, CMD as root with `$ESP $TOP $VAR $SLOT` mounted (see "Editing an installed disk") |
| `journey NAME FILE [--out DIR --from N --keep-going]` | replay steps, screenshot after each, `journey.jsonl`, report |
| `report DIR` | `DIR/report.md` and `DIR/index.html` from a journey dir (or any dir of PNGs) |

`new --drive FILE` (repeatable) adds raw virtio disks after the VM's own, in order (`/dev/vdb`,
`/dev/vdc`...); wolvm never converts them, so a test can compare their bytes after the run (`,ro`
read-only, `,snapshot` writes thrown away). A raw writable extra disk means `snapshot save` of a
running VM fails (savevm needs qcow2 everywhere). `new --no-net` gives no network card (offline
installs). `new --iso-bus virtio` attaches the ISO as a writable throwaway virtio disk
(`snapshot=on`), like a USB stick, instead of a CD drive; works with `--direct`.

`new --disk-bus nvme|sata` puts the VM's own disk on NVMe (`/dev/nvme0n1`) or SATA (`ich9-ahci`,
`/dev/sda`) instead of virtio (`/dev/vda`: change `wolos.autoinstall=` to match); `--disk-lbs`/`--disk-pbs`
set the logical/physical block size (4096/4096 = a 4Kn disk, 512/4096 = 512e); `--extra-bus`,
`--extra-lbs`, `--extra-pbs` do the same for every `--drive`. `--qemu-arg ARGS` (repeatable,
shell-split) is appended last, for anything wolvm has no flag for. `--no-tpm` is the default made
explicit (it wins over a `--tpm` some wrapper passes; `--tpm --no-tpm` together is refused).

Hardware for the QA journeys:
- `--audio N` adds N HDA cards (1: `intel-hda` + `hda-duplex`, 2: + `ich9-intel-hda` + `hda-output`,
  up to 4), all playing into a null audiodev `snd0`. Without it the VM has no sound card (what
  `qa100-dev-no-sound-card.yaml` needs). `snd0` is always there so `plug usb-audio` works on any VM.
- `--displays N` (virtio or virtio-gl only) gives virtio-vga `max_outputs=N` and the id `vga0`,
  and adds `video=Virtual-K:<res>e` for K = 2..N to the console words (heads 2..N start connected:
  with `-display none` nothing else turns them on). Like the console words, these ride SMBIOS
  and are ignored with Secure Boot enforced (use `--direct` there). Screenshot one head with
  `screenshot --head K`; in a journey, `heads: all` (or `heads: 0,2`) on any step takes one per
  head (`NN-name-h0.png`, `-h1.png`...).
- `--s3` adds `-global ICH9-LPC.disable_s3=0` (q35 hides S3 otherwise) for `suspend` / `wake`.
- USB hot-plug needs nothing at `new`: the xhci controller has the id `xhci` (bus `xhci.0`).

`new --from-image DISK` starts from a copy of an existing raw/qcow2 disk (an installed system,
or a prepared ESP such as `tests/wolvm/mok-esp.sh` builds).

`--dry-run` before the command prints the qemu command line (`new`, `start`) or the QMP JSON
(`key`, `type`, `click`, `screenshot`, `stop`, `snapshot`, `qmp`, `plug`, `unplug`, `wake`), the guest
command (`display`, `suspend`, `power-cut`), the host commands (`disk-edit`, `set --secureboot`) or the
numbered steps of a journey (`journey`), and changes nothing.

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
- **Kernel words**: `--append "wolos.try"`, `--append "wolos.autoinstall=/dev/vda"` (disk is virtio: `/dev/vda`). They reach only boots from the ISO; after `set --boot disk` / `boot disk` the installed system gets the console words alone. An autoinstall without `wolos.plan=` makes no user (the installed disk shows only the login window), `wolos.autologin=yes` is live-only, and a user added by hand must be in group `seat` (`docs/modules/vm-e2e.md` of the wolos repo, step 2).

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
checkpoint snapshot-save snapshot-restore eject boot note serial-send qmp plug unplug display suspend
wake frames power-cut disk-edit`. Options for every step: `timeout`, `retry`, `on-fail-restore: TAG`,
`shot: false`, `caption`, `heads: all|0,1`; `ssh`: `expect`, `rc` (number or `any`), `save: file.txt`,
`user`; `wait-text`: `regex`, `new`; `click`: `button`, `double`; `qmp`: `expect` (in the JSON reply);
`plug`: `id`, `ro` (`- plug: usb-storage stick.img`); `frames`: `every` (ms), `for` (s), `head`;
`power-cut`: `delay` (s after the match), `timeout`; `disk-edit`: `expect`, `rc`, `save`, `nbd`,
`key-file`, `read-only`, `snapshot: false`. A value `|` takes the deeper-indented lines below as a
literal block (multi-line shell scripts):

```yaml
- ssh: |
    for p in control-center notifications; do wol shell show $p; sleep 1; done
    echo done
  expect: done
- plug: usb-storage usb-bad.img        # relative to where wolvm runs
  id: usbbad
- display: 2 on                        # Virtual-2 plugged, the next shot has `heads: all`
  heads: all
- suspend
- sleep: 8
- wake
- frames: handoff                      # NN-handoff-000.png ... every 150 ms for 10 s
  every: 150
  for: 10
- power-cut: "files 1[0-9]"            # the install log reached "wolos-install: files 1x"
  delay: 20
```

```yaml
name: live ISO boots to the installer
steps:
- start
- wait-text: "Install WolOS: the first screen of the live image"
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

## Editing an installed disk (tamper tests)

`wolvm disk-edit NAME CMD` is the host-side half of the boot tamper journeys
(`qa100-boot2-*.yaml`): it exports the stopped VM's `disk.qcow2` with `qemu-nbd`, mounts the ESP
(p1) at `$ESP`, and from the btrfs on p2 (opened with `cryptsetup --key-file F` when it is LUKS: the
recovery key, written without a newline) the top level at `$TOP`, `@var` at `$VAR` and `@wolos-a` at
`$SLOT`, all under the VM's own `mnt/`; then runs `sh -c CMD` as root and takes everything down again.

```sh
wolvm disk-edit inst 'echo 99 > $VAR/lib/wolos/secureboot/floor'
wolvm disk-edit inst --key-file rk 'rm -f $ESP/loader/entries/wolos-a.conf'
wolvm disk-edit inst --read-only 'ls -l $ESP/EFI/wolos'
```

Safety, in order: it refuses a running VM (two writers corrupt a qcow2); it refuses an nbd device
already in use (`/sys/block/nbdN/pid`; pick another with `--nbd /dev/nbd3`) and loads `nbd
max_part=16` when there is none; before a writing edit it takes the stopped-VM snapshot
`pre-edit-<time>` of the disk and the vars (`wolvm snapshot NAME restore pre-edit-...` undoes the
edit; `--no-snapshot` skips it); it unmounts, closes the LUKS mapping and disconnects in a
`finally`, also when CMD fails; `--read-only` exports and mounts read-only. It needs root:
it runs its commands through `sudo -n` when not root, so give the user passwordless sudo for
qemu-nbd/mount/cryptsetup or run it as root. CMD is a root shell on the host: quote it, and never
point it at host paths outside `$ESP $TOP $VAR $SLOT`. If a run is killed in the middle (SIGKILL),
clean up by hand: `sudo umount <home>/vms/NAME/mnt/*; sudo cryptsetup close wolvm-NAME;
sudo qemu-nbd -d /dev/nbd0`.

## The QA-100 gate

`tests/run-installed.sh qa100` runs the QA-100 journeys (`qa-*.yaml`, `qa100-*.yaml`) against one
wolvm VM and reports each against `tests/wolvm/qa100.manifest`: per journey its needs (`live`,
`installed` or `iso-install`; `account:NAME`, `network`, `displays:N`, `audio`/`no-audio`, `s3`,
`usb`, `disk-edit`, `tpm`, `secureboot`, `drive`, `ssh`, `release-0120`) and the result expected on
today's release: `pass`, or `fail:<id>` with the BUGS.md id (`.claude/v012/qa100/BUGS.md`) the
journey reproduces (`<report>:<id>` for reports the registry has not merged yet).

```sh
tests/run-installed.sh qa100 --list        # the table, counts per need and per expected result
tests/run-installed.sh qa100 --dry-run     # every journey parses (wolvm --dry-run journey)
tests/run-installed.sh qa100 --vm inst --have installed,account:alice,network,displays:2 \
    --restore desktop --out /scratch/<you>/qa100
```

The expect column is `pass`, `fail:<id>`, `fail:<id>@<steps>` or `fail:<id>@visual`. `@3,7` pins
the step numbers (as `wolvm journey` prints them) whose assertion is the bug; `@visual` says no
step asserts it (the screenshots, saved output or always-passing probes are the evidence). A
journey whose needs `--have` does not cover is SKIP; a run journey ends:

| Result | Meaning |
|---|---|
| PASS / FAIL | expected to pass, and it passed / failed (FAIL: the gate exits 1) |
| XFAIL | failed as expected; for a pinned journey, its first failed step is a pinned one |
| INFRA | pinned, but the first failure is another step (named in the output and the summary): setup, harness or VM state, not the bug |
| XPASS | passed although a failure is expected: the bug may be fixed, check and set `pass` |
| REVIEW | an `@visual` journey ran (passed or failed): a person reads its report |

XPASS and INFRA are fatal only with `--strict`. An unpinned `fail:<id>` still counts any failure as
XFAIL. `--restore TAG` restores a wolvm snapshot before each journey so they start from the same
state; `--only 'qa100-gfx*'` picks a subset. Results: `DIR/qa100-summary.tsv` (with the first
failed step of each journey), one journey dir and log per journey.

## Tests

```sh
python3 tests/wolvm/test_wolvm.py     # step parser, key/QMP builders, qemu command, PNG writer, report, dry runs,
                                      # hardware flags, hot-plug/S3/power-cut/frames/disk-edit steps, every journey
                                      # dry-run, the qa100 manifest covering every qa journey
shellcheck tests/run-installed.sh
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
