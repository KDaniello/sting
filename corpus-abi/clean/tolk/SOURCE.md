# Source

`sting-abi-cli dictionary-build`'s real, deployed Tolk growth corpus — same purpose and
provenance rule as [`../func/SOURCE.md`](../func/SOURCE.md), separate directory because Tolk's
`Extractor` needs a `project_dir`/entrypoint pair, not a single flat file, and its own entrypoint
discovery (`sting_abi_bench::corpus::discover_tolk_projects`) requires an upper-case first letter
on the entrypoint file name (matching this workspace's existing `corpus/clean/acton-contracts`
convention, e.g. `JettonWallet.tolk`) — real upstream files that don't already follow this
convention are renamed on vendoring, noted per entry below, never content-modified.

Deliberately a separate root from `abi-layer bench`'s own two Tolk corpora
(`corpus/clean/{acton-contracts,tolk-bench}`, `crates/sting-abi-cli/src/bench.rs`'s own hardcoded
list) — `dictionary-build` adds this directory as a third root
(`crates/sting-abi-cli/src/dictionary.rs`) without touching `bench`'s own roots, so this can never
affect Block 06's accuracy baseline.

## `offchain-offer-v3/OffchainOfferV3.tolk`

Real, unmodified aside from the file name, fetched byte-for-byte via the GitHub contents API from
[`ton-community/contract-abis`](https://github.com/ton-community/contract-abis) — a mirror of
`verifier.ton.org`'s own verified-source database, indexed by exact code_hash
(`verified/f0/449dca7dd58b13607508839087668196aaef2c7df9ef5fe5b73e0a9177255e/verifier.ton.org/
sources/offchain-offer-v3.tolk`), fetched 2026-07-22. **Renamed from upstream's lowercase
`offchain-offer-v3.tolk`** to satisfy this corpus's entrypoint-discovery convention (content
byte-identical otherwise).

**Why this file specifically**: this exact code_hash
(`f0449dca7dd58b13607508839087668196aaef2c7df9ef5fe5b73e0a9177255e`) is the single largest
still-unrecognized dictionary gap found across two consecutive mainnet sweeps — **366 addresses**,
more than any other candidate in `git show 2ce485e:docs/abi-layer/COVERAGE_SWEEP_REPORT.md` §8's entire history.
Proof of match is exact, not inferred: `verifier.ton.org`'s own `metadata.json` alongside this
source states `"codeCellHash": "f0449dca7dd58b13607508839087668196aaef2c7df9ef5fe5b73e0a9177255e"`
— an independently-verified, cryptographic code-hash match, not a name/interface guess. An
NFT-marketplace "offchain offer" contract (Tolk, compiled with Tolk 1.3.0 per the same metadata).

**Confirmed via a real extraction run**: 1 real get-method — `get_offchain_offer_v2`. Live
`dictionary-probe` against the real flagged address confirms recognition.

## `registry-<code_hash prefix>/*.tolk`

14 directories, from a **full bulk crawl** of `ton-community/contract-abis` (methodology, and why
a bulk crawl rather than more reactive one-file-at-a-time additions, documented in
[`../func/registry/SOURCE.md`](../func/registry/SOURCE.md) — the same crawl produced these, just
filtered to `.tolk` files). Of 100 real `.tolk` files in the registry containing at least one
`get fun`, **17 successfully compiled and extracted** through this workspace's real
`TolkExtractor` (single-file entrypoints, `@stdlib`-only imports — Tolk's compiler is strict about
resolving every import, so files needing project-local sibling imports the crawl didn't also fetch
were expected to fail, same reasoning as the FunC batch's own failures). Each vendored file was
renamed to satisfy this corpus's existing capitalized-entrypoint convention (content byte-for-byte
unchanged) — full traceability (original registry path, code hash, verifier, extracted method
names) in [`../func/registry/`](../func/registry/)'s sibling manifest pattern, recorded here as
`REGISTRY_MANIFEST.json` in this same directory.
