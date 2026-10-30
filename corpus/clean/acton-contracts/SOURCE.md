# Source

Contract sources (each project's `contracts/` subdirectory only — not
`wrappers/`/`scripts/`/`tests/`) from
[`ton-blockchain/acton-contracts`](https://github.com/ton-blockchain/acton-contracts)
(shallow clone, `main` branch, fetched 2026-07-18), all 10 projects: `config`,
`counter`, `dns`, `elector`, `highload-v3.1`, `jetton-v1.3`, `jetton-v2.1`,
`multisig-v2.1`, `nft-v1.1`, `w5.2`.

## Compile verification

Every PascalCase-named entrypoint compiled with `npx @ton/tolk-js <file>`
(Tolk v1.4.2) on 2026-07-18, **except two, documented not silently
dropped**:

- `jetton-v2.1/JettonMinter.tolk` and `multisig-v2.1/Multisig.tolk` (via
  `order-state.tolk`) both `import "@gen/....code"` — Acton's own build-time
  path-mapping feature (auto-generated code-cell reference for cross-contract
  deploy), resolved by the Acton toolchain itself, not by `tolk-js` alone.
  Installing and configuring Acton for 2 files was judged disproportionate to
  this block's scope; both files are still vendored (their sibling files —
  `JettonWallet.tolk`, `Order.tolk` — compile independently and are used for
  merged-per-project analysis, `git show a7280d2:docs/security/blocks/03-benchmark-harness.md`), just
  not counted among the individually-compile-verified entrypoints.

All other entrypoints — `Config.tolk`, `Counter.tolk`, `DnsCollection.tolk`,
`DnsItem.tolk`, `RootDns.tolk`, `Elector.tolk`, `HighloadWalletV3.tolk`,
`NotcoinMinter.tolk`, `NotcoinWallet.tolk` (`jetton-v1.3` — despite the
directory name, this project is Notcoin's jetton fork, not a generic
Jetton), `JettonWallet.tolk` (`jetton-v2.1`), `Order.tolk` (`multisig-v2.1`),
`NftCollection.tolk`, `NftItem.tolk`, `WalletV5.tolk` — compile clean.
