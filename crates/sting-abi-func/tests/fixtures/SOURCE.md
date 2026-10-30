# Source

Real, public FunC source files, fetched 2026-07-21 for
`git show 15b2fe0:docs/abi-layer/blocks/00-bootstrap-shared-core.md` risk spike A ("parse ≥3
real FunC files, check for `contract{}`-style parsing gaps") — used by
`../parse_real_func_samples.rs`, never modified from upstream.

Deliberately spans two eras/styles per that block's own risk table ("FunC
files from different eras... may parse substantially differently"):

| File | Repository | Path | Commit |
|---|---|---|---|
| `wallet3-code.fc` | `ton-blockchain/ton` | `crypto/smartcont/wallet3-code.fc` | `53ec9684bd213983e1fe0f7610d3e3453a4ec628` |
| `stdlib.fc` | `ton-blockchain/ton` | `crypto/smartcont/stdlib.fc` | `c56e65739b66ffad99e29fa46ad0a53bf1583cac` |
| `jetton-wallet.fc` | `ton-blockchain/token-contract` | `ft/jetton-wallet.fc` | `f2253cb0f0e1ae0974d7dc0cef3a62cb6e19f806` |
| `jetton-minter.fc` | `ton-blockchain/token-contract` | `ft/jetton-minter.fc` | `920c5aa3a33ede6405b5653147895c9e27bfe535` |
| `nft-item.fc` | `ton-blockchain/token-contract` | `nft/nft-item.fc` | `31876c659f157df0f5d013b7b33851d1d2a9c3f7` |
| `simple-subscription-plugin.fc` | `ton-blockchain/wallet-contract` | `func/simple-subscription-plugin.fc` | `68b56dc0f7f6a9d41a190a4c91f6dc68b18f0600` |

`simple-subscription-plugin.fc` — added Фаза 2 (`git show c0ae8a9:docs/abi-layer/critical/COVERAGE_SCALE_PLAN.md`,
2026-07-22), a real, previously-unrecognized third storage idiom: a paired `load_storage()`/
`save_storage(...)` wrapper-function pair — structurally identical to `load_data`/`save_data`, but
under a different name, which `storage::extract_named_pair_storage` now recognizes. Pinned
end-to-end in `../real_extraction.rs`.

`wallet3-code.fc` — old (pre-`impure`-keyword-era) wallet contract style,
`#include`-based. `stdlib.fc` — the FunC standard library itself, dense with
`asm`-primitive declarations, the densest stress case tried. The three
`token-contract` files — modern, "impure"-era standard token contracts,
the style the vast majority of real deployed FunC actually looks like today.

Result (recorded here, not just in `ARCHITECTURE.md`, so it stays next to
what produced it): all five parse with **zero** `ERROR` nodes under the
regenerated grammar in `vendor/tree-sitter-func/` — see that directory's own
`README.md` for why the crates.io `tree-sitter-func` v1.0.0 could not be
used directly.

## Corpus-driven audit (Block 04, `docs/abi-layer/blocks/
04-func-heuristic-extractor.md`)

`git show a7280d2:docs/abi-layer/critical/EXTRACTOR_METHODOLOGY.md` §5's required full pass
of every field against the real source, run 2026-07-21, one clean cycle
(zero remaining extractor bugs after the fixes below) — `tests/
real_extraction.rs` pins the result of every row so a regression is caught
automatically:

| File | get-methods found | storage recognized | notes |
|---|---|---|---|
| `jetton-minter.fc` | 2 (`get_jetton_data`, `get_wallet_address`) | yes | `load_data`/`save_data` pair present, arity 4 |
| `jetton-wallet.fc` | 1 (`get_wallet_data`) | yes | same pair form, arity 4, tensor return type |
| `nft-item.fc` | 1 (`get_nft_data`) | **no** | `load_data` exists, but only inline `set_data(...)`, no `save_data` wrapper — the documented v1 boundary, not a bug |
| `wallet3-code.fc` | 2 (`seqno`, `get_public_key`) | **no** | inline `get_data()`/`set_data()` throughout, no wrapper pair at all |
| `stdlib.fc` | 0 | no | utility/`asm`-primitive file, no contract logic at all |

Two real extractor bugs found and fixed during this pass (both empirical,
reproduced and locked in as regression tests — see `src/storage.rs`'s own
test module doc comments for the full detail): a `method_call`'s numeric
argument parses as `parenthesized_expression(number)`, not a bare
`number`, for any real (non-zero) argument — none of the five organic
fixtures' `load_data` bodies happen to use a width-bearing call
(`load_uint(N)`/`load_int(N)`), only zero-arg ones (`load_coins()`,
`load_msg_addr()`, `load_ref()`), so this path was never exercised until
the synthetic fixture below was added specifically to cover it. Separately,
a single-field (non-tuple) `load_data` return parses as a *flat* sequence
of `return_statement`'s own children, not one wrapped expression, when
written without redundant parens (the idiomatic style) — the original
implementation only handled the multi-field tensor-wrapped case.

## Synthetic fixtures

`synthetic-storage-pair.fc` — hand-authored, **not** vendored from a real
deployed contract (see its own header comment). Added specifically to
reach 3 real-shaped `load_data`/`save_data`-pair examples for Level 2
storage's positive coverage (2 organic + 1 synthetic) and, as it turned
out, to exercise a call shape (`load_uint(32)`/`load_int(8)`, a real
bit-width argument) that no organic fixture in this corpus happens to use
— honestly labeled as synthetic per this block's own risk table
("синтетический корпус... честно помеченный как synthetic"), not
presented as further organic evidence.
