# How a release is cut

A release is not a state of the tree. It is a `v*` tag, and everything else follows from it
mechanically — which is why nothing has been released: no tag exists, every crate carries
`version = "0.0.0"` and `publish = false`, and there is no remote to push to
([`../DECISIONS.md`](../DECISIONS.md) §1).

This document describes what happens when that changes, so that the first release is executed rather
than designed under pressure.

---

## 1. What the tag starts

`release.yml` builds `sting-cli` — and only `sting-cli` — for four targets:

| Platform | Target | Runner |
|---|---|---|
| Windows | `x86_64-pc-windows-msvc` | windows-latest |
| Linux | `x86_64-unknown-linux-gnu` | ubuntu-latest |
| macOS arm64 | `aarch64-apple-darwin` | macos-latest |
| macOS x86-64 | `x86_64-apple-darwin` | macos-latest |

`--locked`, so the build uses the committed `Cargo.lock` and a release is reproducible from the tag
rather than from whatever the registry held that day. `fail-fast: false`, so one platform failing
still tells you about the other three.

## 2. What an archive holds

The binary, `README.md`, and `docs/domains/emulator.md` under the name `GETTING_LIBEMULATOR.md`.
Each archive ships with a `.sha256` beside it, computed on the runner that built it.

**`libemulator` is deliberately not bundled.** It is a large, platform-specific binary owned by
`ton-blockchain/ton`, released on its own cadence and under its own licence; bundling it would make
this project the distributor of somebody else's binary and would freeze a copy that goes stale
between releases. The archive therefore ships the instructions instead of the file
([`../domains/emulator.md`](../domains/emulator.md) §4).

## 3. The three surfaces, and why one of them ships

Of the three product surfaces, only the security CLI has a release path. `abi-layer` and the scam
checker build and run from a checkout and have no artefact of their own.

That is a limit rather than an oversight, and it has a reason: `abi-layer` needs the real Tolk and
Tact toolchains vendored to do most of what it does, so a bare binary would fail at the first request
for two of its three languages. The scam checker needs chain access and a real emulator. Shipping
either as a standalone download would ship something that does not work on arrival, and the fix is
the container the service already has rather than a tarball.

## 4. What has to be true before a first tag

| Condition | Why |
|---|---|
| `make pr` green from a clean clone | the tag is built from the tree, not from a working directory |
| a version other than `0.0.0` in `Cargo.toml` | a release without a version is not a release |
| `publish = false` reconsidered per crate | publishing a name is irreversible, and which crates go to a registry is the owner's decision ([`../DECISIONS.md`](../DECISIONS.md) §8) |
| `CHANGELOG.md` carries the tag's entry | written before the tag, since a changelog written afterwards is a summary rather than a record |
| a remote exists | there is nowhere to push a tag today |

## 5. What this does not describe

It does not describe publishing to a registry. That is a separate, irreversible action on a shared
namespace, and no workflow here performs it.

It does not describe versioning policy. The tree is at `0.0.0` and the question of what the first
number means — and whether the three surfaces version together or apart — is open and belongs to the
owner rather than to a document.
