# Source — bulk `verifier.ton.org` registry import (FunC)

Not hand-picked per contract like the flat files in `corpus-abi/clean/func/`'s own `SOURCE.md` —
a **full, mechanical crawl** of
[`ton-community/contract-abis`](https://github.com/ton-community/contract-abis) (a mirror of
`verifier.ton.org`'s verified-source database), pinned at commit
[`fe1d9b690646ef86927dc7dc554e0829139b18ea`](https://github.com/ton-community/contract-abis/commit/fe1d9b690646ef86927dc7dc554e0829139b18ea)
(fetched 2026-07-22).

## Why a bulk import, not a reactive one

Earlier growth rounds (see `../SOURCE.md`) checked this same registry only *reactively* — against
a specific list of code hashes a mainnet sweep had already flagged as unrecognized. Prompted by a
direct question ("if we take mainnet history, can we grow the dictionary faster?"), the real
bottleneck was found to be structural, not a matter of sweeping deeper: within any window of
mainnet traffic, a small set of standard contracts dominates (dedup ratio consistently ×60-90),
so continuing to sweep mostly re-discovers the same already-known codes. The registry itself,
however, is a fixed, finite, already-real set — walking *all* of it once is decoupled entirely from
mainnet-sweep speed.

## Methodology

1. Fetched the registry's entire git tree in one call (`git/trees/main?recursive=true`) —
   20,137 entries, not truncated. Parsed into `(code_hash, verifier, file_path)` groups:
   **1348 distinct verified code hashes**, 1387 (code_hash, verifier) groups with at least one
   real source file.
2. Fetched every `.fc`/`.func` file's raw content via `raw.githubusercontent.com` (a separate host
   from the GitHub API, not subject to its rate limit) — 1207 files.
3. Ran every one through this workspace's own real `FuncExtractor` (the same code path
   `abi-layer dictionary-build` and `abi-layer extract` use, no separate/relaxed logic) —
   **1185 files produced at least one real get-method**, the rest failed for the same reasons
   already documented in `../SOURCE.md` (dispatch-stub-only entrypoints, `#include`-only
   dependency files with no `method_id` of their own, genuine parse errors).
4. Every successful file vendored **unmodified**, at
   `registry/<code_hash>/<verifier-domain>/<original file name>` — full traceability back to the
   exact registry path is preserved in `MANIFEST.json` (this directory), one entry per vendored
   file: `code_hash`, `verifier`, `original_path` (the registry's own path), `vendored_path`,
   and the exact `methods` list this workspace's own extractor found in it — a machine-generated,
   auditable record, not hand-written prose (infeasible at this scale, 1185 files).

## Real, load-bearing findings from the crawl

- **Net new to the dictionary**: 874 method names never seen before this import (dictionary size
  went from 219 to 1077 distinct method ids from the FunC batch alone; 1388 after Tolk+Tact
  batches below).
- **Duplicate code hashes across verifiers are common and harmless**: several contracts are
  verified independently by both `orbs.com` and `verifier.ton.org` — same real bytecode, same
  get-methods, just re-confirmed under a second provenance path. Not deduplicated before
  vendoring (harmless — `dictionary.rs`'s `learn_from_schema` merges identical `(name, id)` pairs
  into one dictionary entry's `seen_in` list regardless).
- **Live validation**: full-corpus `dictionary-build` + `abi-layer sweep --reprobe` against the
  existing `coverage-data/pilot.sqlite3` mainnet sample immediately jumped Tier 3 recognition from
  75.2% to 86.2% of unique on-chain code hashes (391 total) — see
  `git show 2ce485e:docs/abi-layer/COVERAGE_SWEEP_REPORT.md` for the full, dated write-up.
- **Discovery had to become recursive**: `sting_abi_bench::corpus::discover_func_projects`
  previously only scanned `corpus-abi/clean/func/` one level deep (every file there was
  historically hand-picked and flat) — extended to walk subdirectories recursively so this bulk
  import's nested `registry/<hash>/<verifier>/` layout is discovered too, without disturbing any
  existing flat file's `label`/`entrypoint` (same value as before, verified by a new regression
  test in `sting-abi-bench/src/corpus.rs`).

## What this is *not*

Not a substitute for the hand-curated, prose-documented entries in `../SOURCE.md` — those exist
specifically because a real mainnet gap candidate was traced to them one at a time, with
address-level evidence. This bulk import is a different, complementary growth lever: maximum
real coverage from an already-real, already-verified source, at the cost of per-file narrative
provenance (replaced by the manifest's machine-checkable record instead).
