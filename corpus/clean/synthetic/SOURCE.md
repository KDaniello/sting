# Source

**Hand-authored by the sting project, not vendored from any external repo** —
unlike every other subdirectory of `corpus/clean/`. Written 2026-07-18 during
Block 03 (`git show a7280d2:docs/security/blocks/03-benchmark-harness.md`, в истории: `git show cf52463:git show a7280d2:docs/security/blocks/03-benchmark-harness.md`) after the vendored
(`tolk-bench`/`acton-contracts`) and converted (`converted/`) corpora turned
out to structurally under-supply three of the four mutators
(`sting_bench::mutate_lep`/`mutate_id`/`mutate_ubm`) with applicable material:

- Native Tolk code (`tolk-bench`/`acton-contracts`) is close to 100% idiomatic
  struct-based (auto-serialized) storage and message parsing — exactly the
  style `docs.ton.org`'s own Tolk-vs-FunC stdlib table recommends over manual
  `loadXxx`/`storeXxx` calls (see `../../REFERENCES.md#grammar`) — so it
  almost never exercises `LEP`/`ID`'s "manual path".
- The `func2tolk`-converted corpus (`../converted/`) does use manual
  `loadXxx`/`storeXxx` extensively, but its `loadData()`-style storage
  readers return a bare tuple that callers destructure
  (`var (a, b) = loadData();`) rather than binding load calls directly to
  named locals inside one function — exactly the "load/save через
  промежуточный хелпер" shape `git show a7280d2:docs/security/blocks/02-detectors-v1.md`'s own
  Результаты section already named as out of `ID`'s v1 (intra-procedural)
  scope, not a new gap.

Mature, already-reviewed reference contracts are systematically biased
*against* containing the residual patterns these detectors target — that is
part of why they are mature and reviewed. Relying solely on such a corpus for
mutation-engine material would understate detector applicability to less
polished, more common real-world code (exactly what Block 02's own golden
fixtures, `corpus/fixtures/detectors/`, already represent by design). These
three files close that gap honestly: each is a small, realistic contract
using the "manual"/single-function shape the mutators and detectors are
designed for, so `mutate_lep`/`mutate_id`/`mutate_ubm` have real material to
inject into beyond Block 02's existing hand fixtures.

Each file compiles with the real compiler
(`npx @ton/tolk-js <entrypoint>`, Tolk v1.4.2, no `error:` lines) — verified
2026-07-18, same standard as every vendored/converted file in `corpus/clean/`.
Confirmed independently clean (zero findings) by `sting`'s own 4 detectors
before being used as mutation-engine input, exactly like every other
`corpus/clean/` project.

- `wallet-like/` — `WalletLike.tolk` + `storage.tolk`: 3 independent
  `slice`-parsing functions with a correct `.assertEnd()` on every path
  (`LEP` material), a manual `WalletState.load()`/`.save()` pair with
  consistent field order (`ID` material), and a `save()`-then-bounceable-`send()`
  in `onInternalMessage` with a correct, state-restoring `onBouncedMessage`
  (`UBM` material).
- `jetton-manual/` — `JettonManual.tolk`: 3 independent manual
  `loadXxx`/`storeXxx` storage read/write pairs (`saveMinterData`/`loadMinterData`,
  `saveWalletData`/`loadWalletData`, `saveAuctionData`/`loadAuctionData`),
  each with consistent field order — `ID` material.
- `nft-manual/` — `NftManual.tolk`: 2 more `slice`-parsing functions with
  correct `.assertEnd()` (`LEP` material), plus the same
  manual-load/save-then-bounceable-send shape as `wallet-like/` (`UBM`
  material).
