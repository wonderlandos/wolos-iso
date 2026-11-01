# Building the WolOS ISO

**Concept.** The image is a function of two inputs: a published package repo (`[wolos-testing]` or
`[wolos]`, plus the Arch snapshot in `profile/snapshot`) and this repo's commit. Nothing is compiled
here.

**Why.** Like Arch's releng ISO: what users install is exactly what the repo serves, so a test of the
image is a test of the repo, and any image can be rebuilt later from the same two inputs. A dev tree
on someone's disk can never leak into a release.

**How.**

```sh
# With wolos-build next to this repo (github.com/wonderlandos/wolos-build):
../wolos-build/wolos-build iso                       # local mkarchiso, from [wolos-testing]
../wolos-build/wolos-build iso --ecs                 # the same on the ECS cluster
WOLOS_REPO_NAME=wolos ./build.sh                     # by hand, from the stable [wolos]
tests/test-install.sh ~/WolOS-vm/build/out/wolos-*.iso   # boot + install in QEMU
```

Signing (`release/sign-release.sh`, `release/sb-layout.sh sign`) happens only in `release.yml`,
with the keys in GitHub secrets. ADR: wolos `docs/adr/0029-repository-split-and-single-build-path.md`.
