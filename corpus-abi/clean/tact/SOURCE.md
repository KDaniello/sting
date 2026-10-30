# Source

Tier 1 Tact corpus for `git show 43f199c:docs/abi-layer/blocks/06-accuracy-benchmark-harness.md`
(`git show a7280d2:docs/abi-layer/critical/BENCHMARKS.md` §2.1's "Реальные Tact-контракты"
requirement, ≥15 real, diverse contracts). All 19 files below are real,
unmodified, self-contained end-to-end test contracts from
[`tact-lang/tact`](https://github.com/tact-lang/tact)'s own
`src/test/e2e-emulated/` test suite — not hand-written for this repo, not
edited after fetching — fetched byte-for-byte via
`raw.githubusercontent.com`, same non-fabrication principle as
`crates/sting-abi-tact/tests/fixtures/SOURCE.md` and
`corpus/clean/converted/SOURCE.md`. None of the files here duplicate a file
already vendored in `crates/sting-abi-tact/tests/fixtures/` (`getters.tact`,
`message.tact`, `opcodes.tact`, `contract-nested-struct.tact`,
`map-comparison.tact`) — chosen specifically to broaden this repo's total
real-Tact-source coverage, not repeat it.

**Pinned commit** (all 19 files fetched from this exact commit of `main`,
2026-07-21): [`e3d660568eca259b6ae653717e7414e5eac0830e`](https://github.com/tact-lang/tact/commit/e3d660568eca259b6ae653717e7414e5eac0830e)
("chore: add deprecation notice (#3514)"). One pinned commit for the whole
corpus (simpler than Block 00's per-file commit pinning, since every file
here was fetched in one pass from one `git ls-tree` snapshot).

There is no top-level `examples/` directory in `tact-lang/tact` at this
commit (confirmed by listing the repo root tree) — `src/test/e2e-emulated/`
is genuinely the only source of real, compiler-team-authored, individually
compilable Tact contracts in the upstream repo.

## Layout

One subdirectory per contract under `corpus-abi/clean/tact/<name>/`,
containing just the entrypoint `.tact` file — every contract vendored here
turned out to be genuinely single-file after the one candidate that would
have needed a sibling (`contract-upgradable/upgrade.tact` +
`upgradeable.tact`) was dropped (see below), so no subdirectory currently
has more than one `.tact` file — matching `corpus/clean/converted/<name>/`'s
layout convention regardless (siblings would sit flat next to the
entrypoint in the same subdirectory if a future revision adds a
multi-file example). No generated compiler
artifacts (`.abi`/`.code.boc`/`.fc`/`.fif`/`.md`/`.pkg`/`.ts`) are committed
here — every one was produced during compile verification below and then
deleted; the benchmark harness itself regenerates them on demand via
`TactCompiler`.

## Compile verification

Every entrypoint compiled with the real vendored compiler
(`@tact-lang/compiler` v1.6.13, `vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js`,
same package/version already confirmed in
`crates/sting-abi-tact/tests/fixtures/SOURCE.md`), run exactly as:

```sh
node <repo-root>/vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js <entrypoint>.tact
```

with the working directory set to the contract's own subdirectory — same
invocation shape `TactCompiler::compile` itself uses. (No file in the final
19 needed a relative sibling import in the end; several use `@stdlib/*`
imports — `deploy`, `ownable`, `dns`, `stoppable` — which the compiler
resolves from its own bundled stdlib, nothing to vendor.) **19/19 vendored
contracts compiled with zero errors** on
2026-07-21, confirmed by the full artifact set (`.abi`, `.code.boc`, `.fc`,
`.fif`, `.md`, `.pkg`, `.ts`) landing next to each source file; those
artifacts were then deleted, leaving only the `.tact` source(s) in each
subdirectory.

## The 19 contracts (why each was picked)

| Dir | File(s) | Path in `tact-lang/tact` | Feature/pattern demonstrated |
|---|---|---|---|
| `text-message-receivers/` | `text-message-receivers.tact` | `src/test/e2e-emulated/contract-receivers/text-message-receivers.tact` | Multiple string-literal text receivers (including escaped-quote/backslash/unicode literals) plus a catch-all `receive(s: String)` fallback receiver |
| `external-fallbacks/` | `external-fallbacks.tact` | `src/test/e2e-emulated/contract-receivers/external-fallbacks.tact` | `external()`/`external(msg: T)`/`external(s: Slice)` receiver kind — no other file in this corpus (or the existing Block 00/01 fixtures) exercises external message receivers |
| `empty-message-getter-parameter/` | `empty-message-getter-parameter.tact` | `src/test/e2e-emulated/contract-getters/empty-message-getter-parameter.tact` | Minimal getter taking a user-defined message/struct type as its argument and returning the same type |
| `contract-and-abstract-trait-method/` | `contract-and-abstract-trait-method.tact` | `src/test/e2e-emulated/contract-getters/contract-and-abstract-trait-method.tact` | Trait chain with an `abstract fun`, a `virtual fun`, and an `override fun` at each link (`TraitA` → `TraitB` → `contract Test`) |
| `contract-with-init-init-parameter/` | `contract-with-init-init-parameter.tact` | `src/test/e2e-emulated/contract-init/contract-with-init-init-parameter.tact` | `init()` taking a struct-typed parameter (not just scalars) |
| `contract-optional-state/` | `contract-optional-state.tact` | `src/test/e2e-emulated/contract-state/contract-optional-state.tact` | Parametrized contract storage (`contract Test(x: Int)` — Tact's contract-parameter storage-declaration shorthand) and a getter returning `Self?` |
| `contract-methods/` | `contract-methods.tact` | `src/test/e2e-emulated/functions/contract-methods.tact` | Private `self`-methods called from a receiver two different ways (immediate return vs. via a local variable) |
| `masterchain/` | `masterchain.tact` | `src/test/e2e-emulated/masterchain/masterchain.tact` | Masterchain vs. basechain address validation via a free (non-method) function plus two chain-specific text receivers |
| `deploy/` | `deploy.tact` | `src/test/e2e-emulated/stdlib/deploy.tact` | `deploy()`/`initOf`/`DeployParameters` child-contract deployment pattern, three differently-shaped deploy messages |
| `send/` | `send.tact` | `src/test/e2e-emulated/message-sending/send.tact` | `emit()`, `nativeReserve()`, multiple `SendMode` combinations (`SendRemainingBalance`, `SendDefaultMode \| SendPayFwdFeesSeparately`, ...) |
| `traits/` | `traits.tact` | `src/test/e2e-emulated/traits/traits.tact` | 3-level trait inheritance chain (`Animal` → `Dog` → `Laika` → `contract`) plus a separate trait-constant override (`WithConstant`/`IWantToOverrideConstant`) combined via multiple trait inheritance on one contract |
| `base-trait-function-override/` | `base-trait-function-override.tact` | `src/test/e2e-emulated/traits/base-trait/base-trait-function-override.tact` | Overriding a compiler-injected `BaseTrait` method (`notify`) rather than a user-defined trait method |
| `map-traverse/` | `map-traverse.tact` | `src/test/e2e-emulated/maps/map-traverse.tact` | `map<Int, Int>` storage field, a map nested inside a struct field, and `foreach (k, v in m)` traversal — the corpus's main `map<K,V>` exercise (distinct file from the `map-comparison.tact` already vendored in Block 01's fixtures) |
| `local-type-inference/` | `local-type-inference.tact` | `src/test/e2e-emulated/statements/local-type-inference.tact` | Wide sweep of getter return types inferred from `let` bindings — `Address`, `Slice`, `Cell`, `Builder`, `String`, `StateInit`, `map<Int,Int>`, `map<Int, Int as uint32>`, user structs, `Int?` — useful ABI-shape breadth in one file |
| `dns/` | `dns.tact` | `src/test/e2e-emulated/uncategorized/dns.tact` | Real-world-shaped DNS resolver contract using `@stdlib/dns` (`DNSResolver` trait, `doResolveDNS` override) — the closest thing to a "real deployed contract" (vs. a synthetic language-feature test) in this corpus |
| `message-opcode-method/` | `message-opcode-method.tact` | `src/test/e2e-emulated/structs-and-messages/message-opcode-method.tact` | Explicit numeric-literal message opcodes (`message(0x100) Add {}`) plus the `.opcode()` static method exposed as getters — different opcode-declaration style from the Block 00 `opcodes.tact` fixture |
| `inMsg/` | `inMsg.tact` | `src/test/e2e-emulated/structs-and-messages/inMsg.tact` | Four differently-shaped message types on one contract, including a `Slice as remaining` tail field, all routed to distinct typed receivers |
| `serialization-3/` | `serialization-3.tact` | `src/test/e2e-emulated/contract-state/serialization-3.tact` | Every non-Int/Bool primitive TVM-facing type as both a storage field and a getter return type in one place: `Cell`, `Slice`, `Builder`, `String` — type coverage none of the other picks exercise |
| `tact-reserved-contract-errors/` | `tact-reserved-contract-errors.tact` | `src/test/e2e-emulated/exit-codes/tact-reserved-contract-errors.tact` | Multiple `@stdlib` traits combined on one contract (`Ownable`, `Stoppable`), a `const` of type `Address`, and real reserved Tact exit codes (`TactExitCodeNullReferenceException` etc.) surfacing through many text receivers |

## Dropped candidates (not silently omitted)

- **`contract-upgradable/upgrade.tact` + `contract-upgradable/upgradeable.tact`**
  (the `Upgradable` trait — code/data upgrade via `SETCODE`/`setData`). Fetched
  and attempted first as the corpus's example of the upgrade pattern, but
  failed to compile against the vendored v1.6.13 compiler:
  `Error: upgradeable.tact:48:13: Cannot find global function "setData"`.
  `setCode()` is declared locally in the same file as an `asm fun`, but
  `setData()` is called as if it were a compiler-provided global — it isn't,
  in v1.6.13. This is a real version skew: the file was fetched from
  `tact-lang/tact`'s current `main` HEAD, which is ahead of the npm-published
  compiler version this repo vendors (`vendor/tact/README.md`,
  `crates/sting-abi-tact/tests/fixtures/SOURCE.md` — `@tact-lang/compiler`
  itself carries a "Tact is deprecated" notice as of that version, so no
  further npm releases should be expected to close this gap). Not
  hand-patched (this corpus's own rule: never edit fetched source to make it
  compile) — dropped after one failed attempt and replaced with
  `contract-state/serialization-3.tact`, which needed no changes and covers
  a genuinely different, still-uncovered ABI-type-shape gap (`Cell`/`Slice`/
  `Builder`/`String` fields). Every other file in
  `src/test/e2e-emulated/contract-upgradable/` follows the identical
  `setData()` pattern, so the whole subdirectory was avoided, not just this
  one file.
- **Multi-contract-per-file candidates avoided by design, not by failed
  compile** — several otherwise-strong candidates
  (`uncategorized/sample-jetton.tact`, a real TEP-74-shaped Jetton
  minter+wallet in one file; `contract-children/initof.tact`;
  `contract-receivers/bounced-routing.tact`) declare **more than one**
  `contract { ... }` per source file. These compile cleanly with `tact.js`
  itself, but `crates/sting-abi-tact/src/compiler.rs`'s own doc comment is
  explicit that `TactCompiler`/`TactExtractor` v1 "does not support
  multi-contract source files" (`TactcError::MultipleContracts` — the
  extractor can't guess which of several produced `.abi` artifacts the
  caller meant). Since this corpus exists specifically to feed the Block 06
  accuracy-benchmark harness built on top of that same extractor, every file
  actually vendored here declares exactly one `contract { ... }`, confirmed
  by grepping each candidate for `^contract \w` before fetching it into
  place. Flagged here rather than silently avoided, in case a later corpus
  revision wants real multi-contract coverage once v1's single-contract
  limitation is lifted.

## Summary

**19/19 vendored contracts compiled clean** with
`node vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js <entrypoint>.tact`
(v1.6.13, 2026-07-21), zero errors. 1 candidate pair dropped for a genuine
compiler-version mismatch (documented above, not a corpus gap — a different,
equally useful file was substituted). 19 clears the target of 15 Tier 1
contracts with margin.
