# Source

> **`registry/` subdirectory**: a full, mechanical bulk import of the entire `verifier.ton.org`
> registry (1185 files, 874 net-new methods) — see [`registry/SOURCE.md`](registry/SOURCE.md) for
> methodology; everything below this line is the earlier, hand-picked, one-file-at-a-time growth
> history that predates it.

`sting-abi-cli dictionary-build`'s FunC growth corpus — real, deployed, unmodified `FunC`
contracts fed through `FuncExtractor` to learn additional real `(name, tvm_method_id)` pairs into
`corpus-abi/method-dictionary.json`. Separate from `crates/sting-abi-func/tests/fixtures/`
(extractor *correctness* fixtures, some synthetic) — everything here is real, deployed, chosen
specifically because a real Фаза 1 mainnet sweep (`git show 2ce485e:docs/abi-layer/COVERAGE_SWEEP_REPORT.md`)
found it as an unrecognized `dictionary_gap_candidate`, not picked arbitrarily.

## `governed-jetton-wallet.fc`

Real, unmodified, fetched byte-for-byte via `raw.githubusercontent.com` from
[`ton-blockchain/stablecoin-contract`](https://github.com/ton-blockchain/stablecoin-contract)
(`contracts/jetton-wallet.fc`), **pinned commit**
[`5a3b500267b0bdfc6505a08e5ac661c805cab8b0`](https://github.com/ton-blockchain/stablecoin-contract/commit/5a3b500267b0bdfc6505a08e5ac661c805cab8b0)
(fetched 2026-07-22). TON Foundation's own reference implementation of the "governed jetton
wallet" (TEP-74 + TEP-89, admin-lockable) standard real USD₮-on-TON wallets use.

**Why this file specifically**: the pilot sweep found its top
dictionary gap candidate — code_hash `89468f02c78e570802e39979c8516fc38df07ea76a48357e0536f2ba7b3ee37b`,
**99 real addresses** (~10% of the whole 998-address sample), zero recognized Tier 3 methods.
`toncenter.com`'s own address-book API resolves a real address running this code
(`0:D9C9EE7C5B23C4D1B83E1EC8C009364CF986016CF8BA9F0B4F8BAD465E982BE6`) to interface
`jetton_wallet_governed`, and its `metadata` endpoint confirms the same address is a real jetton
wallet for `Tether USD` (USD₮) — this is a real USDT-on-TON jetton wallet, not a guess.

**Confirmed via a real extraction run** (`FuncExtractor`, this same commit): 2 real get-methods —
`get_wallet_data` (`tvm_method_id 97026`, same id the plain TEP-74 `jetton-wallet.fc` fixture
already contributes — same name, same deterministic `crc16`-derived id, not a new collision) and
`get_status` (`tvm_method_id 100881`, genuinely new — the one addition this standard makes beyond
plain TEP-74, an admin-lockable wallet's freeze/pause status). Also has a real, literal
`load_data()`/`save_data(...)` storage pair (4 fields: `status`/`balance`/`owner_address`/
`jetton_master_address`) — recognized by the existing paired-form extractor unchanged, a bonus
real validation data point, not the reason this file was chosen.

`#include`d dependencies (`stdlib.fc`/`op-codes.fc`/`workchain.fc`/`jetton-utils.fc`/`gas.fc`) are
**not** vendored — `FuncExtractor` never expands `#include`, and this file's own
`recv_internal`/get-methods/storage logic is fully self-contained without them (confirmed: the
real extraction above ran directly against this one file, no missing-symbol failure).

## `nft-fixprice-sale-v4r1.fc`

Real, unmodified, fetched byte-for-byte via `raw.githubusercontent.com` from
[`getgems-io/nft-contracts`](https://github.com/getgems-io/nft-contracts)
(`packages/contracts/sources/nft-fixprice-sale-v4r1.fc`), **pinned commit**
[`7f2cf76ea4dbe6b84da99796adc8fcb80d5a7e4e`](https://github.com/getgems-io/nft-contracts/commit/7f2cf76ea4dbe6b84da99796adc8fcb80d5a7e4e)
(fetched 2026-07-22). Getgems (TON's largest NFT marketplace) own real fixed-price NFT sale
contract, v4r1.

**Why this file specifically**: the pilot's second-ranked real (non-library-cell) dictionary gap
candidate — code_hash `6b95a6418b9c9d2359045d1e7559b8d549ae0e506f24caab58fa30c8fb1feb86`, 16
addresses. `tonapi.io`'s own interface auto-detection resolves a real address running this code
(`0:B01E60B5AB47FDDB612F8BABADECD9FBDB82751F05BA0928F701555AB7E23401`) to interface
`nft_sale_getgems_v4`, and names its own known get-method `get_fix_price_data_v4` — matched exactly
by this file's real, and only, `method_id`-marked function.

**Confirmed via a real extraction run**: 1 real get-method — `get_fix_price_data_v4`
(`tvm_method_id 122483`), genuinely new to the dictionary (no prior entry). `#include`d
dependencies (`imports/stdlib.fc`/`op-codes.fc`) not vendored, same reasoning as above.

## `multisig.func`

Real, unmodified, fetched byte-for-byte via `raw.githubusercontent.com` from
[`ton-blockchain/multisig-contract-v2`](https://github.com/ton-blockchain/multisig-contract-v2)
(`contracts/multisig.func`), **pinned commit**
[`9a4b13df6345c9c4068ca725e434b40f9ea5ca28`](https://github.com/ton-blockchain/multisig-contract-v2/commit/9a4b13df6345c9c4068ca725e434b40f9ea5ca28)
(fetched 2026-07-22). TON Foundation's own official multisig wallet standard, v2. Vendored under
its real upstream `.func` extension (not renamed to `.fc`) — `discover_func_projects` accepts
both, `.func` being `.fc`'s real, live alternative some `FunC` projects use, this the first real
example this workspace's corpus needed it for.

**Why this file specifically**: another real (non-library-cell) dictionary gap candidate from the
pilot — code_hash `d3d14da9a627f0ec3533341829762af92b9540b21bf03665fac09c2b46eabbac`, 3 addresses.
`tonapi.io`'s own interface auto-detection resolves a real address running this code
(`0:D42F37D8107BA011E2FFDF6243AA295564D2D3B9A94E0DD5EE94D09569B35F29`) to interface `multisig_v2`,
naming its own known get-method `get_multisig_data` — matched exactly by this file.

**Confirmed via a real extraction run**: 3 real get-methods — `get_multisig_data`
(`tvm_method_id 107307`, zero-argument, probeable), `get_order_estimate` (`93855`, 2 parameters,
recorded but not probeable — `sting-abi-dictionary`'s own documented v1 scope, see that crate's
module doc comment), `get_order_address` (`111399`, 1 parameter, same). `#include`d dependencies
(`imports/stdlib.fc`/`types.func`/`op-codes.func`/`errors.func`/`messages.func`/
`order_helpers.func`) not vendored, same reasoning as above.

## `stonfi-vault-v2.fc`, `stonfi-router-v2-get.fc`, `stonfi-pool-v2-get.fc`, `stonfi-lp-wallet-v2.fc`, `stonfi-lp-account-v2-get.fc`

Real, unmodified, fetched byte-for-byte via the GitHub contents API from
[`ston-fi/dex-core-v2`](https://github.com/ston-fi/dex-core-v2), **pinned commit**
[`af0a955cc835af9697cd383e201fefcbe1a6a87e`](https://github.com/ston-fi/dex-core-v2/commit/af0a955cc835af9697cd383e201fefcbe1a6a87e)
(fetched 2026-07-22). STON.fi's own real, fully open-source (confirmed live — STON.fi publishes
all its contracts and states deployed bytecode is directly comparable to this repo) DEX contracts,
v2 — TON's largest AMM DEX by volume alongside DeDust.

**Why these files specifically**: a follow-up mainnet sweep (`coverage-data/pilot.sqlite3`) found
several real, non-library-cell dictionary gaps whose code `tonapi.io` itself already labels
`stonfi_vault_v2`/`stonfi_router_v2`/`stonfi_escrow_vault` — real STON.fi DEX contracts that were
simply absent from our dictionary, not unknown to the wider ecosystem.

**Real project layout, not a single self-contained file**: `dex-core-v2`'s top-level
`contracts/{router,vault,pool,lp_wallet,lp_account}.fc` are thin `recv_internal` dispatch stubs
that `#include` the actual logic from same-named subdirectories — `FuncExtractor` never expands
`#include` (documented limitation, see entries above), so the dispatch stubs alone would yield
*zero* get-methods. Instead, each subdirectory's own `get.fc` (containing the real, `method_id`
get-method definitions) is vendored directly, renamed with a `-get` suffix for clarity; `vault.fc`
and `lp_wallet.fc` needed no such split — their get-methods are defined in the top-level file
itself, vendored unchanged. `deployer.fc` (no get-methods, pure deploy helper) and the top-level
dispatch-only `router.fc`/`pool.fc` were fetched but not vendored — zero extraction value.

**Confirmed via real extraction runs**, one genuinely new get-method each beyond what the plain
jetton/NFT standards already contributed: `get_vault_data` (`stonfi-vault-v2.fc`), `get_pool_address`/
`get_vault_address`/`get_router_data`/`get_upgraded_pool_code`/`get_router_version`
(`stonfi-router-v2-get.fc`), `get_pool_type`/`get_lp_account_address`
(`stonfi-pool-v2-get.fc` — its `get_jetton_data`/`get_wallet_address` re-confirm existing TEP-74
entries, same `crc16`-derived ids, not new), `get_wallet_data` (`stonfi-lp-wallet-v2.fc`, likewise
a re-confirmation), `get_lp_account_data` (`stonfi-lp-account-v2-get.fc`). Note:
`get_pool_type()`'s body returns a template placeholder string (`"<%= dexType %>"`) — the upstream
repo templates a handful of pool variants (constant-product vs. stable-swap) from these shared
sources before compiling; irrelevant to us, since dictionary entries only depend on the *function
name* (hence `tvm_method_id`), never its body's literal return value.

## `nft-fixprice-sale.fc`, `nft-fixprice-sale-v2.fc`, `nft-fixprice-sale-v3.fc`, `nft-fixprice-sale-v3r2.fc`, `nft-fixprice-sale-v3r3.fc`, `nft-auction-v1-get.fc`

Real, unmodified, fetched byte-for-byte from
[`getgems-io/nft-contracts`](https://github.com/getgems-io/nft-contracts)
(`packages/contracts/sources/nft-fixprice-sale{,-v2,-v3,-v3r2,-v3r3}.fc` and
`packages/contracts/sources/nft-auction/struct/get-met.func`), same **pinned commit** as
`nft-fixprice-sale-v4r1.fc` above
([`7f2cf76ea4dbe6b84da99796adc8fcb80d5a7e4e`](https://github.com/getgems-io/nft-contracts/commit/7f2cf76ea4dbe6b84da99796adc8fcb80d5a7e4e)).
Getgems's own full historical lineage of fixed-price sale contract versions, plus its first
auction contract version.

**Why these files specifically**: the same follow-up sweep's forensics found a real gap whose code
`tonapi.io` labels `nft_sale_getgems_v3` (not `v4` — the version we already had) and another
labelled `nft_auction_v1`. Rather than guess which exact historical `v2`/`v3`/`v3r2`/`v3r3` variant
matches the live bytecode, all four (plus the plain `v1`, same repo, same commit, negligible extra
cost) were vendored — real, genuinely different historical contracts Getgems actually deployed, not
speculative variants — and dictionary growth + a live `dictionary-probe` against the real flagged
address (§ below) settles which one(s) actually match.

**`nft-auction-v1-get.fc`** is `nft-auction/struct/get-met.func` specifically, not the top-level
`nft-auction/nft-auction.func` entrypoint — same `#include`-splitting reasoning as the STON.fi
entries above: the entrypoint's own get-methods live in this included file.

**Confirmed via real extraction runs**: `get_sale_data` (present, same name/id, across all five
fixprice variants — one dictionary entry, five real `seen_in` sources) plus `get_fix_price_data`
(new, `nft-fixprice-sale-v3r3.fc` only). `nft-auction-v1-get.fc` contributes 9 real get-methods:
`get_nft_owner`, `get_nft_addr`, `get_last_member`, `get_mp_addr`, `get_mp_fee_addr`,
`get_royalty_fee_addr`, `get_fees_info`, `get_bid_info`, `get_sale_data` (same name/id as the
fixprice contracts' own — a real, deterministic `crc16` collision across two genuinely different
Getgems contract families, not a bug).

## `nominator-pool.fc`

Real, unmodified, fetched byte-for-byte via the GitHub contents API from
[`ton-blockchain/nominator-pool`](https://github.com/ton-blockchain/nominator-pool)
(`func/pool.fc`), **pinned commit**
[`2f35c36b5ad662f10fd7b01ef780c3f1949c399d`](https://github.com/ton-blockchain/nominator-pool/commit/2f35c36b5ad662f10fd7b01ef780c3f1949c399d)
(fetched 2026-07-22). TON Foundation's own reference validator nominator-pool contract — not found
via a mainnet gap this time, added proactively: real, canonical TON core infrastructure (every
validator that accepts outside stake runs an instance of this), self-contained (its own vendored
`stdlib.fc` not needed for extraction), and absent from the dictionary despite covering
`Elector`/`Config`/`HighloadWalletV3`/`Multisig`/`WalletV5` already.

**Confirmed via a real extraction run**: 7 real get-methods — `get_pool_data`,
`has_withdraw_requests`, `get_nominator_data`, `get_max_punishment`, `list_nominators`,
`list_votes`, `list_voters`, all genuinely new to the dictionary.

## `collection-offer-v2.fc`, `ton-storage-contract.fc`, `orbs-nominators.fc`, `dragonz-land-diamonds.fc`, `orbs-pool.func`, `orbs-deployer.fc`, `swapcoffee-vault-jetton.fc`, `swapcoffee-factory.fc`

Real, unmodified (aside from a descriptive rename away from generic upstream file names like
`get.fc`/`constants.fc`/`pool.func`, documented per entry), each fetched byte-for-byte via the
GitHub contents API from [`ton-community/contract-abis`](https://github.com/ton-community/contract-abis)
(`verifier.ton.org`'s or `orbs.com`'s own verified-source mirrors, indexed by exact code_hash),
fetched 2026-07-22 — same discipline as `offchain-offer-v3/OffchainOfferV3.tolk` above: every one
of these files' presence was confirmed by an **exact `metadata.json` `codeCellHash` match** against
a real mainnet gap candidate (`git show 2ce485e:docs/abi-layer/COVERAGE_SWEEP_REPORT.md` §8.2's forensics run
programmatically checked all ~106 then-unrecognized code hashes against this registry — 14 exact
matches found, most vendored here; a couple of very-low-value single-address matches with only
already-known method names, e.g. a second STON.fi pTON-wallet variant contributing only the
already-covered `get_wallet_data`, were checked but not vendored — no new dictionary value).

- **`collection-offer-v2.fc`** (from `verified/a6/91f2556d.../verifier.ton.org/sources/contracts/
  collection-offer-v2.fc`) — the **second-largest** gap candidate ever found, **230 addresses**. An
  NFT-collection-wide marketplace offer contract, "immediate swap" variant (per its own header
  comment). 2 get-methods: `get_collection_offer_data` (new), `get_nft_address_by_index`
  (re-confirms an existing TEP-64 collection entry).
- **`ton-storage-contract.fc`** (from `verified/38/5a5f0ddb.../orbs.com/sources/
  storage-contract.fc`, Orbs-verified) — a real TON Storage-provider contract (proves it's still
  hosting a bag-of-files, claims rewards) — **55 addresses**. 4 new get-methods: `get_storage_info`,
  `get_provider_info`, `get_available_balance`, `get_providers`.
  `constants.fc` (same directory, fetched alongside) has no get-methods of its own, not vendored.
- **`orbs-nominators.fc`** (from `verified/1f/0c196801.../orbs.com/sources/nominators.fc`) — a
  third-party (Orbs Network's own) validator nominator-pool implementation, distinct from
  `nominator-pool.fc` (TON Foundation's own, above) — real, different bytecode, richer getter
  surface. 12 new get-methods: `get_proxy`, `get_owner`, `get_controller`, `get_unowned`,
  `get_available`, `get_staking_status`, `get_pool_status`, `get_params`, `get_member_balance`,
  `get_members_raw`, `get_members`, `get_member`.
- **`dragonz-land-diamonds.fc`** (from `verified/81/c4f794c1.../orbs.com/sources/
  dragonz_land_telegram_diamonds_source.fc`) — a small, self-contained sales contract. 6 new
  get-methods: `get_nonce`, `get_owner`, `get_sales_active`, `get_processing_price`, `get_pricing`,
  `get_price`.
- **`orbs-pool.func`** (from `verified/19/2535677e.../orbs.com/sources/pool.func`) — an Orbs
  liquid-staking/bridge pool contract (validator-loan accounting, `controller`/`loan` vocabulary).
  7 new get-methods: `get_controller_address` (+ a `_get_controller_address` forward declaration),
  `get_controller_address_legacy`, `get_loan`, `get_pool_full_data`, `get_pool_full_data_raw`,
  `get_controller_loan_position`, `calculate_loan_amount`.
- **`orbs-deployer.fc`** (from `verified/b7/84d77f7e.../orbs.com/sources/smc/deployer.fc`) — a
  small deploy-helper contract. 1 new get-method: `is_supported_hash`.
- **`swapcoffee-vault-jetton.fc`**, **`swapcoffee-factory.fc`** (from
  [`swapcoffee/dex`](https://github.com/swapcoffee/dex), **pinned commit**
  [`4d3c1ca6d2898919ec5a8f410caf5f08033917d4`](https://github.com/swapcoffee/dex/commit/4d3c1ca6d2898919ec5a8f410caf5f08033917d4)
  — swap.coffee's own DEX, a different, real, open-source AMM from STON.fi/DeDust, identified via
  its literal `0xc0ffee20`/`0xc0ffee21` opcode constants matching a real gap candidate's observed
  transaction pattern exactly (`contracts/include/constants.fc`), not a code_hash-exact match like
  the entries above — the specific vault contract instance backing that one candidate wasn't
  pinned down byte-for-byte, but the get-method *names* below are real, extracted, and independent
  of that uncertainty. `vault_jetton.fc`: `get_asset`, `is_active` (both new). `factory.fc`: 8 new
  get-methods — `get_vault_address`, `get_pool_address`, `get_pool_address_no_settings`,
  `get_pool_creator_address`, `get_pool_creator_address_no_settings`,
  `get_liquidity_depository_address`, `get_liquidity_depository_address_no_settings`,
  `get_admin_address`, `get_code`. Two of these (`get_vault_address`, `get_pool_address`) share a
  `tvm_method_id` with STON.fi's own same-named router methods but a *different* parameter
  count — a real, deliberately-preserved dictionary collision (`dictionary-build` reports it, not
  silently merged), matching this codebase's "don't guess which signature is correct" discipline.

## `ton-core-*.fc`, `payment-channel-async.func`, `eth-bridge-*.fc`, `whales-nominator-pool-get.fc`

Proactive TON-core-infrastructure growth (not tied to a specific mainnet gap candidate this
round — direct instruction to "go into TON's own core and find more gaps for the dictionary too").
Every file below is real, unmodified, fetched byte-for-byte via the GitHub contents API, fetched
2026-07-22.

- **`ton-core-{highload-wallet,highload-wallet-v2,restricted-wallet,restricted-wallet2,
  restricted-wallet3,simple-wallet-ext,dns-auto-code,dns-manual-code,pow-testgiver-code}.fc`** —
  from [`ton-blockchain/ton`](https://github.com/ton-blockchain/ton) (the actual TON node
  monorepo), `crypto/smartcont/*.fc`, **pinned commit**
  [`ca7d6e456745a64eac8204a7b0b64516b61cf2db`](https://github.com/ton-blockchain/ton/commit/ca7d6e456745a64eac8204a7b0b64516b61cf2db).
  This directory is the actual canonical reference source for most of TON's original, still
  widely-deployed contract families — highload wallets (common on exchanges/bots), restricted
  wallets (team/vesting-locked funds, 3 versions), `.ton` DNS auction/registrar contracts
  (`dns-auto`/`dns-manual`, distinct from the Tolk `DnsItem`/`DnsCollection`/`RootDns` already
  covered), and the historical PoW-testgiver contract from TON's original mining giveaway. New
  get-methods: `balance`, `balance_at`, `wallet_id` (restricted wallets); `getexpirationx`,
  `getexpiration`, `getstdperiod`, `getppr`, `getppc`, `getppb`, `calcprice`, `calcregprice`
  (dns-auto); `get_contract_id` (dns-manual); `create_init_state`,
  `prepare_send_message_with_seqno`, `prepare_send_message`, `verify_message` (simple-wallet-ext);
  `get_pow_params` (pow-testgiver). `config-with-ownable-params.fc` and `payment-channel-code.fc`
  (same directory) were checked and **not** vendored — the former's get-methods (`get_proposal`/
  `list_proposals`/`proposal_storage_price`) already fully covered by `Config.tolk`, the latter has
  no `method_id`-marked functions at all (superseded by the newer `payment-channels` repo below).
- **`payment-channel-async.func`** — from
  [`ton-blockchain/payment-channels`](https://github.com/ton-blockchain/payment-channels)
  (`func/async-channel.func`), **pinned commit**
  [`e605580c3fb1feb22d80be9a0cddfcd05671c347`](https://github.com/ton-blockchain/payment-channels/commit/e605580c3fb1feb22d80be9a0cddfcd05671c347).
  TON's real, official payment-channel primitive — searched for twice before in this same corpus's
  history and not found quickly enough to justify the time; found this round. 2 new get-methods:
  `get_channel_state`, `get_channel_data`.
- **`eth-bridge-{jetton-bridge,jetton-minter,jetton-wallet,multisig,votes-collector}.fc`** — from
  [`ton-blockchain/token-bridge-func`](https://github.com/ton-blockchain/token-bridge-func)
  (`src/func/jetton-bridge/*.fc`), **pinned commit**
  [`4e7ec44a651e6b455ce5a09ed1383535fae3a637`](https://github.com/ton-blockchain/token-bridge-func/commit/4e7ec44a651e6b455ce5a09ed1383535fae3a637).
  TON Foundation's own real ETH↔TON token bridge. New get-methods: `get_minter_address`,
  `get_bridge_data`, `get_wrapped_token_data` (bridge-specific jetton variant); and a genuinely
  different, older multisig standard (`multisig.fc`, the bridge's own governance multisig,
  distinct from `multisig-contract-v2` already vendored) — `get_query_state`, `processed?` (shares
  an id/name with `HighloadWalletV3`'s own `processed?`, different signature, a real, harmless
  collision — dictionary correctly keeps both, neither individually probeable),
  `get_public_keys`, `check_query_signatures`, `message_signed_by_id?`, `messages_by_mask`,
  `get_messages_signed_by_id`, `get_messages_unsigned_by_id`, `get_messages_unsigned`, `get_n_k`,
  `merge_inner_queries`, `get_lock_timeout`; plus `get_external_voting_data` (`votes-collector.fc`).
- **`whales-nominator-pool-get.fc`** — from
  [`tonwhales/ton-nominators`](https://github.com/tonwhales/ton-nominators)
  (`sources/modules/get.fc`), **pinned commit**
  [`0553e1b6ddfc5c0b60505957505ce58d01bec3e7`](https://github.com/tonwhales/ton-nominators/commit/0553e1b6ddfc5c0b60505957505ce58d01bec3e7).
  Ton Whales' own nominator-pool implementation — one of TON's largest, most widely-deployed
  staking pool codebases, a third distinct implementation alongside TON Foundation's
  `nominator-pool.fc` and Orbs' `orbs-nominators.fc` above. Nearly the same getter surface as
  Orbs' version (same real convention, likely a shared lineage/fork), re-confirming those ids under
  a second, independent, arguably more widely-deployed provenance; contributes exactly one
  genuinely new method beyond what Orbs' file already gave: `supported_interfaces`.

## Researched, declined — no genuine open-source match found

Same discipline as declining DeDust's `get_asset` above — a real, live `tonapi.io`/`toncenter.com`
interface label is not by itself sufficient; every entry here still needs a real, fetchable,
compilable source file.

- **DeDust `dedust_v2_cpmm`** — DeDust (`github.com/dedust-io`) ships only a TypeScript SDK, no
  FunC contract source repository found.
- **Storm Trade `storm_position_manager`** (Tsunami-Exchange org) — `tsunami-contracts` (their only
  repo with a `contracts/` directory) turned out to be an old Waves-blockchain (RIDE-language)
  project, unrelated despite the name; no TON FunC source repository found anywhere in the org.
- **`cron`**, **`airdrop_interlocker_v2`** — real, live code hashes (confirmed:
  `airdrop_interlocker_v2`'s documented code hash in `tonkeeper/tongo`'s `abi/schemas/airdrop.xml`
  matches our own flagged candidate exactly), but the only "source" found for either is
  hand-reverse-engineered ABI catalogs (`tonkeeper/tongo`, `i582/abis`) — real, useful for
  *identifying* a contract, but not compilable source, and not an acceptable dictionary-entry
  origin under this project's own rules (`MethodDictionary::learn_from_schema` only accepts a real
  extracted `ContractSchema`). Left open — the method names are known (`get_cron_info`;
  `get_distribution_info`/`get_unlocks_info`/`get_available_claim_amount`) for whoever later finds
  or writes a genuine, verifiable source match.
- **`wallet_preprocessed_v2`** — real, fully identified (code hash confirmed against 5+
  independent sources, including official `ton-blockchain/docs`), pinned commit
  [`pyAndr3w/ton-preprocessed-wallet-v2@d95a25e`](https://github.com/pyAndr3w/ton-preprocessed-wallet-v2/commit/d95a25ef7e259c27a07e185296b57335c2b4974b)
  — but the source is raw **Fift assembly** (`pw-code.fif`), not FunC/Tolk/Tact; none of this
  workspace's extractors parse Fift, so it structurally cannot go through
  `MethodDictionary::learn_from_schema`. Separately: a live `dictionary-probe` against the real
  flagged address returns **all entries `inconclusive`**, not a clean miss — this wallet's
  `recv_internal` is `SETCP0 IFNOTRET` (accepts any internal message unconditionally, no op
  dispatch at all), which appears to confuse ordinary TVM get-method probing structurally, a
  deeper mismatch than a missing dictionary entry. Left open.
- **DeDust `github.com/dedust-io/fee-collector`** — genuinely real, open, compilable **Tolk**
  source (`contracts/fee_collector.tolk`, unlike the DeDust vault/pool core above) — fetched and
  inspected, but it defines **zero get-methods** (a pure message-handling contract, no `get fun`
  anywhere in 215 lines) — nothing to extract, not a rule violation, just no dictionary value here.
- **swap.coffee-adjacent DeDust `payout`/`swap` cluster**, **HOT Protocol bridge cluster**,
  **DeDust "Uranus" meme launcher** — each has a real, named, corroborated project (via opcode
  constants matching a real GitHub repo), but in every case the only fetchable repository ships
  compiled-bytecode-consuming SDKs/wrappers (`.ts`/`.gen.ts`), not FunC/Tolk/Tact source — same
  "real project, no compilable source" outcome as DeDust/Storm above, not vendored.
