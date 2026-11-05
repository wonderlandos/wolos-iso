# Building the WolOS ISO

**Concept.** The image is a function of two inputs: a published package repo (`[wolos-testing]` or
`[wolos]`, plus the Arch snapshot in `profile/snapshot`) and this repo's commit. Nothing is compiled
here.

**Why.** Like Arch's releng ISO: what users install is exactly what the repo serves, so a test of the
image is a test of the repo, and any image can be rebuilt later from the same two inputs. A dev tree
on someone's disk can never leak into a release.

**How.** One build path: `wolos-build iso` runs `build.sh` (mkarchiso on the host, no container
fallback), on the forge or with `--ecs` on the cluster. `build.sh` refuses a repo whose published
`snapshot` differs from `profile/snapshot`, and writes the ISO, `<iso>.sha256` and the manifest
`<iso>.json` into the output folder (by hand: `$WOLOS_VM/out`, default `~/WolOS-vm/out`).

```sh
# With wolos-build next to this repo (github.com/wonderlandos/wolos-build), on the forge:
../wolos-build/wolos-build iso --out DIR             # from [wolos-testing]
../wolos-build/wolos-build iso --ecs --out DIR       # the same on the ECS cluster
WOLOS_REPO_NAME=wolos ../wolos-build/wolos-build iso --out DIR   # from the stable [wolos]
tests/test-install.sh DIR/wolos-*.iso                # unattended install + boot (wolvm, KVM)
```

**Release.** `wolos-build release --iso DIR vX.Y.Z` publishes DIR's one ISO with its `.sha256`,
`release.json` and `release.json.sig`. The VM test runs on exactly those artefacts first:

```sh
tests/test-install.sh --release DIR    # checks the ISO against release.json (else <iso>.json) and
                                       # <iso>.sha256, then installs and boots it; BOOT PASS|FAIL
                                       # lines in $WOLOS_VM/test/checks.txt
```

The image has no size limit. `build.sh` prints the size (`release/iso-size.sh`) and never fails on
it. The primary download is the CDN (`https://cdn.wolos.org/v<version>/<iso>`, the `image` of
`release.json`); an ISO of 2 GiB or more is not uploaded to the GitHub release at all (GitHub takes no
asset that size), whose notes link the CDN ISO. That needs the CDN upload token
(`WOLOS_CDN_UPLOAD_TOKEN`); without it `wolos-build release` stops before signing.

The VM tests run on the forge (KVM, QEMU, swtpm, OVMF, virt-firmware) through wolvm
(`tests/wolvm/wolvm`, [tests/README-wolvm.md](tests/README-wolvm.md)); without `/dev/kvm` they run
on TCG with longer timeouts. How a person checks the download: README "Verify the download".

Signing (`release/sign-release.sh`, `release/sb-layout.sh sign`) happens only in the release
path, with the keys in GitHub secrets. ADR: wolos `docs/adr/0029-repository-split-and-single-build-path.md`.
