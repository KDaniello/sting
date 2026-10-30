# What CI runs

CI is not a second definition of the gate. It calls the same `make` targets a developer calls, so
that "green here, red there" cannot happen through the two drifting apart
([`../TESTING.md`](../TESTING.md) §8 says what each workflow guards; this says how they are built).

Seven workflows. One is the gate; six exist because the gate deliberately runs offline, and each of
the six buys back one thing the offline gate cannot see.

---

## 1. The gate

`ci.yml` on every pull request and on a push to `main` or `master`.

| Job | Platforms | What it runs |
|---|---|---|
| lint | ubuntu | `make lint` |
| test | ubuntu, windows | `make test`, then `make test-doc` |

The lint job installs `cargo-deny`, `typos-cli` and Python 3.11; the test job installs
`cargo-nextest`. Nothing else is fetched, because `make lint` and `make test` are the offline gate
by construction: no compiler is vendored, no library is downloaded, no network is touched.

**Two platforms are not decoration.** Windows is the development platform and Linux is where fuzzing
and releases happen. Path separators, dynamic-library names and line endings differ between them,
and this tree carries vendored binaries and byte-exact snapshots — the class of defect that only
appears on the other platform is real here rather than theoretical.

A superseded run is cancelled: the newest push on a branch is the only one whose answer anybody acts
on.

## 2. The six that pay for what the gate cannot see

| Workflow | Trigger | What it buys |
|---|---|---|
| `emulator.yml` | a change under the emulator crates | fetches `libemulator` at the pinned tag on ubuntu and windows and runs the **real** tests, not a compile |
| `tolkc.yml` | a change under the compiler wrapper | fetches the vendored Tolk compiler and its stdlib and runs a real compilation |
| `abi-bench.yml` | a change under an extractor | vendors the real `tolk` and Tact toolchains and runs the extraction benchmark |
| `bench.yml` | a change under the detectors, the corpus or the bench harness | precision and recall against `corpus/baseline.json` |
| `fuzz.yml` | a change under the emulator or fuzz crates, and weekly | sixty seconds per target on a change; a long weekly run that keeps its corpus |
| `release.yml` | a `v*` tag | [`release.md`](./release.md) |

**Every fetch is pinned to a tag rather than to "latest".** The tag is not typed into the workflow:
each job reads `release` out of `[package.metadata.libemulator]` in
`crates/sting-emulator-sys/Cargo.toml`, which is where the pin lives
([`../domains/emulator.md`](../domains/emulator.md) §4). A tag spelled in three workflows beside a
commit spelled in a manifest is one fact in four places, and four places drift. The Tolk compiler
comes from the tag its tests were developed against.

**The fetched build is checked, not trusted.** Loading refuses any `libemulator` but the pinned
commit, so a release that silently republished an asset fails the run by name instead of moving a
measurement. Both platform assets of `v2026.06` were confirmed to report the same commit
`3f598aa2f0911317b626f4093177241e61798c61` — the Windows library directly, the Linux one through a
container.

**The bench harness is a trigger path, not only the detectors it measures.** A scoring defect in the
harness is as much a source of a false green as a defect in a detector, and the harness is not in the
literal list the gate was first written from.

## 3. Why the fuzz smoke run cannot fail the build

`corpus/fixtures/fuzz/BalanceLedger.tolk` is deliberately vulnerable: it is what proves the harness
finds real defects rather than merely running. It reproduces its seeded crash within the first
iterations of any run. A hard-failing smoke step would therefore fail every pull request
unconditionally and never signal a regression — so the step is informational, and the artefact upload
is what carries whatever it found to a human.

Protection against *losing* that seeded bug does not live in the fuzz run. It lives in a plain
deterministic unit test in `crates/sting-fuzz`, which the offline gate already runs.

## 4. What CI does not guard

It does not run the live tests. Anything that needs the network, a container or a key from `.env`
stays behind `make test-live` and behind an `#[ignore]`, and a run of it is a human's decision.

It does not guard the projects that have not merged yet. `karman/` is
outside the workspace, and no workflow here compiles them.

It does not measure performance. Regression in speed or memory is caught by the benchmarks a human
runs, and no workflow fails on a timing.
