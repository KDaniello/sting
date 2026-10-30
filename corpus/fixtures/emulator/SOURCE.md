# Source

`lp-jetton-master.{code,data}.boc.base64` — fetched live from toncenter v3 API
(`/api/v3/account`) for address
`0:000011A50B2CB855DD55972EA27E8FBB76C3EAF135918723849B0E5BDB69EAE6`
(a DeDust LP jetton master, discovered via `/api/v3/jetton/masters`) on
2026-07-17, used as a real-world fixture for git show a7280d2:docs/security/blocks/00-bootstrap.md risk
spike B. Ground truth for `get_jetton_data()` on this contract, from
toncenter's own `runGetMethod`, at the same block:

- exit_code: 0
- stack depth: 5
- stack[0] = num 0x3e8 (1000, total_supply)
- stack[1] = num -0x1 (-1, mintable = true)
- stack[2..5] = cell (admin address slice, content, jetton wallet code)
- gas_used: 45890

`ston-fi-pool.{code,data}.boc.base64` — fetched live from toncenter v3 API
(`/api/v3/account`) for address
`EQD8TJ8xEWB1SpnRE4d89YO3jl0W0EiBnNS4IBaHaUmdfizE` (a real, currently active
STON.fi USD₮/TON pool, found via GeckoTerminal) on 2026-07-23, used as a
real-world fixture for `git show df78785:docs/research/jetton-scamcheck-tz.md` Фаза 2 (LP
concentration). This pool contract is itself a TEP-74 jetton master for its
own LP token — ground truth for `get_jetton_data()` on this contract, from
toncenter's own indexed `/api/v3/jetton/masters` at query time:

- total_supply: 55183210687 (fits the 64-bit tiny int form)
- mintable: true
- admin_address: `0:779DCC815138D9500E449C5291E7F12738C23D575B5310000F6A253BD607384E`
  (the pool's own LP-jetton admin — a STON.fi-controlled address, not
  necessarily the same as any individual liquidity provider)

Same `MYADDR`/`set_c7` requirement as `lp-jetton-master` above (`exit code 9:
cannot parse a MsgAddress` otherwise) — real `set_c7` parameters used by
`crates/sting-scamcheck/tests/ston_fi.rs`, all fetched live for this exact
address on 2026-07-23: `address = EQD8TJ8xEWB1SpnRE4d89YO3jl0W0EiBnNS4IBaHaUmdfizE`
(`/api/v3/account`'s own address, parsed then re-hex-formatted),
`balance = 5237739227200` (`/api/v3/account`), `unixtime = 1784766721`
(the `now` field of this account's latest transaction, `/api/v3/transactions`).

**`get_pool_data()`'s field order turned out to be version-dependent, found
empirically, not assumed**: `docs.ston.fi` documents a v2 schema
(`is_locked, router_address, total_supply, ...`) and a shorter v1 schema
without `total_supply` embedded at all — this specific pool decoded index 2
as a non-`Int` value under the assumed v2 schema, contradicting it (i.e.
this is very likely a v1 pool). `sting-scamcheck`'s Фаза 2 does not call
`get_pool_data()` at all as a result — `total_supply` comes from
`get_jetton_data()` instead (index 0, version-independent, the same call
and decoder already proven in Фаза 1), sidestepping the version-schema
question entirely rather than guessing which version this pool is.

`usdt-holder-wallet.{code,data}.boc.base64` — fetched live from toncenter v3
API (`/api/v3/account`) for address
`0:00000dc46f93a5b60d8f8933ccb567e9064de322e4348da6fd35a7bb2de44a44` (a real
USD₮ jetton-wallet holder, found via `/api/v3/jetton/wallets?jetton_address=
<USD₮>&sort=desc`) on 2026-07-23, used by
`crates/sting-scamcheck/tests/honeypot.rs` and `src/main.rs`'s own CLI for
`git show df78785:docs/research/jetton-scamcheck-tz.md` Фаза 4 (honeypot probe). Ground truth
at fetch time: `owner = 0:88F51DE0DF12207257C6AAFD6112E0CA6781E564BE8FBEB3914B875A63222249`,
`balance = 14539900`, `unixtime = 1729780530` (`last_paid`/latest transaction
`now`), `lt = 50234963000001` (latest transaction `lt`).

**Real, disclosed limitation found via this exact fixture, not a guessed
one**: this wallet's code hash
(`89468F02C78E570802E39979C8516FC38DF07EA76A48357E0536F2BA7B3EE37B`) is
registered in the mainnet config's `ConfigParam 45` as a TON "precompiled
contract" (`docs.ton.org/foundations/precompiled` — real validators run it
as native C++, not interpreted TVM bytecode, because of its transaction
volume). Probing it via emulated `TransactionEmulator` — even with 100% real
on-chain code+data, no synthetic storage at all — reverts with exit code 9
("cannot parse a `MsgAddress`") at VM step 2, because the emulator's own
raw-bytecode fallback for an unrecognized precompiled contract genuinely
cannot execute it correctly. This is *not* evidence of an actual transfer
restriction — `sting_scamcheck::honeypot` detects this structurally, before
ever running the emulator: it decodes the real mainnet config's
`ConfigParam 45` (`HashmapE 256 PrecompiledSmc`) and checks the probed
wallet's own `cell_hash()` against that registered set
(`is_registered_precompiled_contract`), reporting
`ProbeOutcome::PrecompiledContractUnsupported` rather than misreporting
either a false-positive `Reverted` or a false-negative `Executed`. (An
earlier attempt matched `vm_log` text for `"Unknown precompiled contract"`
instead — that line turned out to come from `libemulator`'s own internal
diagnostic logger, never from the structured `vm_log` this module actually
receives via FFI, so it never matched; see `honeypot.rs`'s own doc comment
on `ProbeOutcome::PrecompiledContractUnsupported` for the full correction.)

`ston-fi-lp-holder-wallet.{code,data}.boc.base64` — fetched live from
toncenter v3 API (`/api/v3/account`) for address
`0:612B9FCA34014506EE76D58C7A8D4E0C765171115412652AF4FB8DE4F77622C2` (a real
STON.fi LP-jetton holder of the same pool documented above, found via
`/api/v3/jetton/wallets?jetton_address=<pool-as-LP-master>&sort=desc`) on
2026-07-23 — fetched specifically as a **non**-precompiled control, to
isolate whether the real-holder honeypot-probe approach itself is sound (it
is: this probe succeeds cleanly) versus the USD₮ fixture's failure being
specific to precompiled contracts (it is). Ground truth at fetch time: `owner
= 0:C0FE7EAE7E328D5D19C8DFBD8E7E592FC7DEA5B90B982BAD72959B571CF861D7`,
`balance = 54332047944`, `unixtime = 1784655644`, `lt = 91656448000021`. A
probe transfer within this balance returns `ProbeOutcome::Executed` (real
exit code 0, real gas usage); a probe transfer exceeding it returns
`ProbeOutcome::Reverted` for a real, contract-enforced reason — see
`crates/sting-scamcheck/tests/honeypot.rs`.

`nft-collection.{code,data}.boc.base64` / `nft-item.{code,data}.boc.base64` — fetched live from
toncenter v3 API (`/api/v3/account`) on 2026-07-23, used by
`crates/sting-scamcheck/tests/nft_authenticity.rs` for
`git show 1f54f83:docs/research/gift-nft-trust-layer-tz.md` Фаза 1 (collection-authenticity signal).

- Collection: `0:0018A3FFE171C064F0E1321718976D068C6134AA9B4CA77191709F71BDF30C14`
  (`EQAAGKP_4XHAZPDhMhcYl20GjGE0qptMp3GRcJ9xvfMMFLSZ`), a real, currently active `getgems-io/
  nft-contracts`-shaped TEP-62 collection ("Teddy Brix"), found via `/api/v3/nft/collections`
  filtered to `next_item_index` between 1 and a few thousand (this one: 5). **Not a confirmed
  Telegram Gift collection specifically** — a generic getgems.io collection, used only to
  ground-truth the TEP-62 `get_collection_data()`/`get_nft_data()` call+decode path itself; the
  separate curated registry of known-good *Gift* collection addresses
  (`git show 1f54f83:docs/research/gift-nft-trust-layer-tz.md` §1.1) is a different, product-data concern, not this
  fixture's job. `get_collection_data()` ground truth from toncenter's own indexed response at
  fetch time: `next_item_index = 5`, `owner_address =
  0:52992B41F808E13A824AB9A0957DD464F8C32CAC95E71ABA9978852C30E8BF74`, `collection_content` is
  off-chain (TEP-64 prefix `0x01`), URI `https://s.getgems.io/nft/c/69637b02b57084ca3c10f72b/meta.json`.
  Latest transaction at fetch time: `now = 1768130245`, `lt = 65677143000003`.
- Item #0 of that collection: `0:095DF3E6023C31DD6E5E9ADDB56BFF857B4BB8BA3DEEDC85D82E49F3C6E82609`
  (`EQAJXfPmAjwx3W5emt21a_-Fe0u4uj3u3IXYLknzxugmCd7m`), found via `/api/v3/nft/items?
  collection_address=<the collection above>`. Ground truth: `index = 0`, `collection_address` =
  the collection above, `owner_address =
  0:52992B41F808E13A824AB9A0957DD464F8C32CAC95E71ABA9978852C30E8BF74` (same as the collection's
  own `owner_address` at fetch time — this collection's items had not yet been sold on), semi-chain
  `individual_content` (off-chain URI relative to the collection's own base URI, per TEP-64/62's
  "content of an NFT item is often relative to the collection's base URI" convention), resolved URI
  `https://s.getgems.io/nft/c/69637b02b57084ca3c10f72b/0/meta.json`.

**No real testnet deployment used for the synthetic-copycat ground truth** (§7.1 of the trust-layer
TZ) — same reasoning as `honeypot.rs`'s own probes: a copycat is built entirely as a synthetic
code+data pair (this real collection's own code, a hand-built data cell with the same
`collection_content` but reachable only at a different, never-deployed address) and run purely
inside the local `TvmEmulator`, never sent to any real chain, testnet included.

`mainnet-config.boc.base64` — fetched live from toncenter v2 API
(`/api/v2/getConfigAll`) on 2026-07-18, used as a real (not hand-rolled or
empty) blockchain config `BoC` for Block 04 (`git show a7280d2:docs/security/blocks/04-emulator-ffi.md`, в истории: `git show cf52463:git show a7280d2:docs/security/blocks/04-emulator-ffi.md`)'s
`TransactionEmulator`/`EmulatorConfig` tests
(`crates/sting-emulator/tests/integration.rs`) — specifically so those tests
exercise `transaction_emulator_create`/`emulator_config_create`/
`*_set_config_object` against genuine ~109 KiB config data, not a
minimal/synthetic stand-in that could hide a real parsing bug.
