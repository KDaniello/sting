# Source

Full native-Tolk contract sources (`contracts_Tolk/`) from
[`ton-blockchain/tolk-bench`](https://github.com/ton-blockchain/tolk-bench)
(shallow clone, `main` branch, fetched 2026-07-18), all 7 families —
`01_jetton`, `02_nft`, `03_notcoin`, `04_sharded_tgbtc`, `05_wallet-v5`,
`06_vesting`, `07_telemint`. `contracts_FunC/`, `bench-snapshots/`,
`fift-output/`, `tests/`, `wrappers/` not vendored — only the Tolk source is
needed for `sting bench`'s clean corpus (`git show a7280d2:docs/security/blocks/03-benchmark-harness.md`
§3.1).

`corpus/fixtures/tolk-bench/` (Block 00/01, 5 flat files) is a **separate,
smaller** vendoring of the same upstream repo's `01_jetton` family, used by
`sting-ast`/`sting-pipeline`'s own unit/integration tests — not replaced by
this directory, which exists specifically for the accuracy-benchmark corpus
(`git show a7280d2:docs/security/critical/BENCHMARKS.md` §2.1) and is analyzed per-project (all sibling
files of a family merged into one view), not per-file like `sting lint`.

## Compile verification

Every PascalCase-named entrypoint file compiled successfully with
`npx @ton/tolk-js <file>` (Tolk v1.4.2, matching `../../../docs/REFERENCES.md#grammar`'s
already-verified version) on 2026-07-18:

- `01_jetton`: `JettonMinter.tolk`, `JettonWallet.tolk`
- `02_nft`: `NftCollection.tolk`, `NftItem.tolk`
- `03_notcoin`: `JettonMinter.tolk`, `JettonWallet.tolk`
- `04_sharded_tgbtc`: `JettonMinter.tolk`, `JettonWallet.tolk`
- `05_wallet-v5`: `WalletV5.tolk`
- `06_vesting`: `VestingWallet.tolk`
- `07_telemint`: `TelemintCollection.tolk`, `TelemintItem.tolk`

12/12 entrypoints across all 7 families compile clean, no manual fixes
needed — unlike the `converted/` corpus (see its own `SOURCE.md`), tolk-bench
is native Tolk with `import` statements already correct.
