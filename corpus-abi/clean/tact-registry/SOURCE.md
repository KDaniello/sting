# Source — bulk `verifier.ton.org` registry import (Tact)

129 directories, from the same **full bulk crawl** of
[`ton-community/contract-abis`](https://github.com/ton-community/contract-abis) documented in
[`../func/registry/SOURCE.md`](../func/registry/SOURCE.md) (pinned commit
`fe1d9b690646ef86927dc7dc554e0829139b18ea`, fetched 2026-07-22) — this directory holds the `.tact`
results. Of 709 real `.tact` files in the registry containing at least one `get fun`, **137
successfully compiled and extracted** through this workspace's real `TactExtractor` (a lower
success rate than the FunC batch's ~98% — Tact's compiler is strict about resolving every
`import "./sibling.tact"`, and most real Tact projects split contract logic across several local
files the crawl didn't also fetch, so only single-file-sufficient entrypoints — mostly `@stdlib`
imports only — succeeded). 129 unique directories from 137 manifest entries: the 8 "duplicates"
are the same real code hash independently verified by two different services (`orbs.com` and
`verifier.ton.org`), not data loss.

**Deliberately not inside `corpus-abi/clean/tact/`** — that directory is shared with `abi-layer
bench --corpus` (Block 06's Tier 1 accuracy baseline, `corpus-abi/baseline.json`). Mixing in
129 bulk-imported, unvetted contracts (no hand-written `expected_schema.json`, mixed real-world
quality) would have silently changed what that baseline measures. `crates/sting-abi-cli/src/
dictionary.rs` walks this directory as a second, separate Tact root specifically to keep dictionary
growth and accuracy benchmarking from ever sharing a corpus — same reasoning already applied to
`corpus-abi/clean/tolk/` for the Tolk growth batch.

Full traceability (original registry path, code hash, verifier, extracted method names) in this
directory's own `MANIFEST.json`, one entry per vendored file — machine-generated and auditable,
not hand-written prose (infeasible at this scale).

**Net effect on the dictionary**: this batch, combined with the FunC and Tolk registry batches,
took the dictionary from 219 to **1388** distinct method ids in one session, and live
`--reprobe` validation against the existing `coverage-data/pilot.sqlite3` mainnet sample from
75.2% to **86.2%** of unique on-chain code hashes recognized — see
`git show 2ce485e:docs/abi-layer/COVERAGE_SWEEP_REPORT.md` for the full, dated write-up.
