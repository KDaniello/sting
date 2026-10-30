# The emulator

The FFI to `libemulator`, memory ownership, the static-to-dynamic bridge, and what emulation does not
reach.

---

## 1. Why this is not "just a binding"

A survey of the library's ABI, done before implementation, established the fact that decides this
part's value: **all three existing third-party bindings leak**. None of them releases the JSON strings
the library returns through the destructor the header requires.

Separately, one of the library's own entry points leaks **inside the C++ implementation** — confirmed
by reading its source — and is routed around by calling the detailed variant instead.

For a tool that runs the emulator in a loop over a corpus and in long fuzzing sessions, that is not a
matter of hygiene but the difference between working and eating memory within the hour.
`sting-emulator` is the one binding that does not grow on a long run, and that is a measurable
difference rather than a side effect of tidiness.

---

## 2. Two crates, different rights to `unsafe`

| Crate | `unsafe` | The requirement |
|---|---|---|
| `sting-emulator-sys` | unrestricted | the raw FFI boundary; every function is either unsafe itself or holds an unsafe block around a raw pointer call |
| `sting-emulator` | permitted | **no public function may be `unsafe fn`** |
| every other crate | forbidden | without exception |

The first phrasing of the rule — unsafe only in the raw crate — was unfollowable: a safe wrapper
physically cannot call an unsafe function without an unsafe block of its own. It is the standard pair
of a system binding and its wrapper.

Every unsafe block carries an immediately preceding safety comment; the lint that requires it is
denied rather than warned.

**A hole in that rule was found by use rather than foreseen.** The only way to *load* the library was
an unsafe function in the raw crate. That worked while the only callers were that crate's own tests.
The moment the fuzzing crate — which has no unsafe exception — became the first outside consumer, the
hole was real. It is closed by a safe loader that asserts the same trust boundary **once, here**,
rather than by an exception granted to every new consumer further down the graph.

---

## 3. Dynamic loading instead of linking

The Windows release of the library ships **without an import library**. Link-time binding is therefore
impossible, and the library is loaded at run time.

The constraint was forced and bought two things:

- the path comes from an environment variable, and a test without it **skips rather than fails** —
  otherwise a developer without the vendored library could run nothing at all;
- the platform-specific file name is resolved in one place rather than by a conditional at every call
  site.

A failure to resolve a symbol is allowed to be fatal **only in the constructor** — fail fast if the
library is incompatible — and has to carry its justification in the documented error section. It is the
one exception to the workspace prohibition, and it is named rather than silent.

---

---

## 4. Getting the library

`ton-blockchain/ton` publishes it as a release asset, one per platform, and this tree pins the tag
`v2026.06` — the same tag the fixtures under `corpus/fixtures/emulator/` were generated against. Not
"latest": a release that changes the ABI would otherwise change a measurement without anybody
touching the code.

| Platform | Asset |
|---|---|
| Linux x86-64 | `libemulator-linux-x86_64.so` |
| Linux arm64 | `libemulator-linux-arm64.so` |
| macOS arm64 | `libemulator-mac-arm64.dylib` |
| macOS x86-64 | `libemulator-mac-x86-64.dylib` |
| Windows | `libemulator.dll` |

```sh
curl -sL --fail -o vendor/libemulator/libemulator.so   https://github.com/ton-blockchain/ton/releases/download/v2026.06/libemulator-linux-x86_64.so
```

**The tag is not the pin.** A release tag names a build, and what the code checks is the build's own
commit: `[package.metadata.libemulator]` in `crates/sting-emulator-sys/Cargo.toml` holds
`3f598aa2f0911317b626f4093177241e61798c61`, the build script compiles it in, and loading refuses any
other library by name, both commits stated. The commit is the version because TON's emulator moves
per commit rather than per release; the tag is provenance.

| Field | Value |
|---|---|
| `commit` | `3f598aa2f0911317b626f4093177241e61798c61` |
| `date` | 2026-07-15 13:49:38 +0300 |
| `release` | `v2026.06` |

The manifest is the only place the pin is written. A constant in the source next to a table in the
manifest would be one fact spelled twice, and two spellings drift.

The loader looks at `STING_LIBEMULATOR_PATH` first and otherwise at the platform's conventional file
name — `libemulator.so`, `.dylib` or `.dll` — next to the running executable. The asset name is
therefore dropped on download: it encodes a platform the file name already implies.

Without the library, `sting lint` and `sting bench` are unaffected, every test that needs it skips
rather than fails, and `sting scan` refuses to start with the missing path named.

## 5. Which semantics a run emulates

An emulation is a statement about semantics, and semantics belong to a global
version. The version is not a setting this tree chooses: it is written in the
chain config, parameter 8, and `sting_emulator::global_version_of` reads it
there.

**The vendored mainnet config carries global version 14**, and every emulation
in this tree has been running against it. `PINNED_GLOBAL_VERSION` is that
number, read rather than chosen — the first value written into that constant was
a guess of 11, and the test against the real fixture corrected it on the first
run.

A config carrying a different version is refused by name, both numbers stated, on every path a
config takes into the emulator: `TransactionEmulator::new`, `set_config`, `EmulatorConfig::new` and
`set_c7`. A version checked at one entry point and not the next is a version nobody checked.
"Wrong version" without saying which is a message nobody can act on.

Reading it costs **1.5 ms per config** in release, paid once per emulator creation and never per
message: every caller that runs get methods in a loop passes no config to `set_c7`.

**Mainnet is one version ahead.** Global version 15 went live on 2026-07-23 and
this tree emulates 14. What 15 changed — libraries restricted to governance,
action fines collected for every successful message when the action phase fails,
a cap on total outgoing bits and cells — is ruled against the detectors in
[`detectors.md`](./detectors.md) §9, and none of them moves. The pin is
deliberate rather than neglected: moving it is a step with its own before-and-
after measurement, and the detectors' verdicts are what says the step is safe to
take.

`sting doctor` prints this number alongside the emulator's own commit, the
compiler's version and the grammar's, and exits non-zero when any of the four is
unknown.

---

## 6. The public surface

```rust
load_library(&Path) -> Result<LibEmulator, LoadError>   // safe, the one entry point
method_id(name: &str) -> i32                            // a get-method name to its numeric id

TvmEmulator::new(...)          // get methods and message sending
  .run_get_method(...) / .send_internal_message(...) / .send_external_message(...)
  .set_c7(...) / .set_libraries(...) / .set_gas_limit(...) / .set_config_object(...)

TransactionEmulator::new(...)  // full transaction emulation
  .emulate_transaction(...) / .emulate_tick_tock_transaction(...)
  .set_unixtime(...) / .set_rand_seed(...) / .set_ignore_chksig(...)
```

Only safe types leave the crate: a typed result and the three response structures. No unsafe contract
leaks to a user of the crate.

Setting libraries is not a luxury: without resolving library cells, part of the real contract
population does not execute at all. The cost of getting that wrong is measured in
[`method-dictionary.md`](./method-dictionary.md) §4.

---

## 7. The bridge from static to dynamic, and its limit

The bridge takes a static finding, builds a real scenario from a hypothesis, runs it against live
bytecode and returns one of three outcomes ([`API.md`](../API.md) §1.2).

**A hypothesis is written by a person rather than inferred by the tool.** That is not a corner cut:
deriving a scenario from source is the general problem of recovering a message dispatch convention, an
openly unsolved research question. Guessing it would produce plausible but wrong scenarios — the worst
possible result for a check.

**A confirmed limit, recorded as inconclusive rather than disguised as refuted**: findings that hinge
on the message sender are **unreachable** through the emulator's internal-message entry point by
construction. How many more classes sit in the same position has not been counted, and that is
[`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §5.

**Refuted does not delete a finding.** The report lowers its rendered severity. Deleting it would hide
from the user both that the pattern is in the code and that it was checked.

---

## 8. Precompiled contracts: a boundary of the virtual machine itself

One widely held wallet is registered as a precompiled contract in the chain's configuration. Its raw
bytecode **cannot run under an emulated machine at all**, even on entirely real code and data.

**The first attempt to detect that was wrong and was caught by an integration test.** The check
matched text in the virtual machine's log — but that line comes from the library's **global logger**
rather than from the structured log returned across the FFI. It was checking something other than
what it appeared to.

**Replaced by a structural check**: the relevant configuration parameter is decoded from the real
mainnet config through a generic dictionary decoder, and the probed contract's cell hash is compared
against that set **before** the emulator is started. The outcome is its own variant rather than a
false "reverted".

The difference is practical: before the fix, the risk checker printed a false verdict on the largest
jetton in the network.

---

## 9. Fuzzing

The fuzzing crate holds targets that use the emulator directly, in process. The CLI's fuzz command is
**a thin wrapper over the fuzzing tool** rather than an implementation: it needs a full checkout and a
nightly toolchain with that tool installed.

**The sanitiser built into the fuzzing tool covers the FFI boundary for free** on every run. Fuzzing
here is not only a search for defects in target contracts but regression protection for the raw
binding itself.

Linux only: the sanitiser path on Windows is effectively unusable. CI runs sixty seconds per target on
a change and a long scheduled run that keeps its corpus and its crashes as artefacts.

**The honest boundary of the fuzzing crate**: its body, fixture and harness modules are hard-wired to
one demonstration contract. It is **not** a general message-construction engine — which is exactly why
the risk checker deliberately does **not** depend on it: reusing it would first require generalising
it, and that is a change to a core rather than reuse.

---

## 10. What this document does not cover

It does not describe what is emulated for: which findings the bridge confirms is
[`detectors.md`](./detectors.md), and what the emulator is used for outside the security contour is
[`method-dictionary.md`](./method-dictionary.md) and [`scamcheck.md`](./scamcheck.md). And it makes no
claim about performance: the emulator's cost per run is a measurement that has not been taken.
