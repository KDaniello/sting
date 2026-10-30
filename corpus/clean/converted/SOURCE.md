# Source

Contracts in this directory did **not** start life as Tolk. Each is a real
FunC reference contract, run through the official converter
(`@ton/convert-func-to-tolk`), then manually fixed until it actually compiles
with the real Tolk compiler (`@ton/tolk-js`, Tolk v1.4.2 — matching the
version already verified elsewhere in this repo, see
`../tolk-bench/SOURCE.md`). Per `git show a7280d2:docs/security/blocks/03-benchmark-harness.md` §3.1,
the converter's output is **not** trusted blindly: every file here was
verified by actually running `npx @ton/tolk-js <entrypoint>` from its final
location in this directory and confirming the output contains no `error:`
line. Source repos were shallow-cloned (or fetched directly for single files)
into a scratch directory on 2026-07-18; only the `.tolk` output is vendored
here — no `.fc` originals, no test/wrapper files.

13/13 candidate projects below compile clean. One additional candidate
(`highload-wallet-v2-code.fc`) was tried and dropped — see the bottom of this
file.

## Recurring fix patterns (applies across most files below)

The converter (`@ton/convert-func-to-tolk --warnings-as-comments`) does two
things reliably wrong for these multi-file FunC projects, discovered by
actually compiling, not assumed:

1. **No `import` statements.** These projects were originally compiled by
   passing multiple `.fc` files to the `func` compiler at once (whole-program
   compilation) — no `#include`. The converter has no way to know files are
   related, so every `.tolk` file it writes is missing the `import "sibling";`
   lines it would need on its own. Every entrypoint below needed
   `import "sibling-name";` (no `.tolk` suffix) added for each sibling file it
   calls into, and — because Tolk requires **transitive** imports, unlike
   FunC's flat compilation unit — some sibling files themselves needed an
   `import "params";` etc. added too, not just the entrypoint (e.g.
   `jetton-utils.tolk` and `discovery-params.tolk` both call `workchain()`,
   defined in `params.tolk`; found via the compiler's own
   `hint: forgot to import "params.tolk"?`).

2. **`address` vs `slice` is FunC-loose, Tolk-strict.** FunC treats an
   `address` as just another `slice`; Tolk has a distinct `address` type
   (same bits at the TVM level, but not implicitly interchangeable). This
   shows up as three recurring, mechanical fixes, applied wherever the
   compiler flagged them:
   - a function declared to load/return `slice` for what is actually an
     address (via `.loadAddress()`) needs its signature changed to `address`;
   - a `slice`-only method needed on an `address` value (e.g.
     `.preloadUint()`, `.remainingBitsCount()`) needs an explicit
     `(expr as slice)` cast first;
   - a get-method whose declared TVM-ABI return type is `slice` (TEP-62/TEP-74
     etc. fix the wire format) but that now internally holds an `address`
     needs `expr as slice` at the `return` site.

3. **FunC's `int` as boolean vs Tolk's `bool`.** FunC freely uses `int` (0 /
   -1) in `if (...)` conditions and bitwise `~`/`&` for logical
   not/and. Tolk requires an actual `bool` in conditions. Every
   `if (someFlag)` on an `int` became `if (someFlag != 0)`, and `~x` (bitwise
   NOT used as logical NOT on a 0/-1 int) became `x == 0`.

Only fixes *beyond* these three recurring patterns are called out
individually per file below.

---

## `ft/` — jetton reference contracts (TEP-74)

Source: [`ton-blockchain/token-contract`](https://github.com/ton-blockchain/token-contract),
`ft/` directory, `main` branch, fetched 2026-07-18.
Command (run once from `ft/`, converts the whole folder together):

```
npx --yes @ton/convert-func-to-tolk --warnings-as-comments .
```

All four `jetton-minter*`/`jetton-wallet` entrypoints share the same fixed-up
sibling files: `jetton-utils.tolk`, `discovery-params.tolk`, `op-codes.tolk`,
`params.tolk`. Sibling fixes (apply to all four projects below, each of which
vendors its own copy of these four files):

- `jetton-utils.tolk`: added `import "params";` (needs `workchain()`); in
  `calculateJettonWalletAddress()`, appended `as address` to the
  `beginCell()...beginParse()` return expression (declared return type is
  `address`, `.beginParse()` yields `slice`).
- `discovery-params.tolk`: added `import "params";` (needs `workchain()`);
  `isResolvable()`'s `return wc == workchain();` produces a `bool`, but the
  function is declared `-> int`, so changed to
  `return (wc == workchain()) as int;`.

### `jetton-minter-discoverable/`

Original: [`ft/jetton-minter-discoverable.fc`](https://github.com/ton-blockchain/token-contract/blob/main/ft/jetton-minter-discoverable.fc)
(the file's own header comment recommends this variant over `jetton-minter.fc`).

Fixes beyond the recurring patterns and shared-sibling fixes above:
- `loadData()`'s declared return type was `(int, slice, cell, cell)`; the
  second element is actually loaded with `.loadAddress()`, so changed to
  `(int, address, cell, cell)`. This return type mismatch is what then forced
  three call-site fixes for consistency: `get_jetton_data()`'s return became
  `(totalSupply, -1, adminAddress as slice, content, jettonWalletCode)`
  (the get-method's TEP-74 ABI keeps `slice`, even though internally it's now
  `address`).
- `onInternalMessage`: `responseAddress.preloadUint(2)` — `responseAddress` is
  `address`, but `.preloadUint()` is a `slice`-only method — changed to
  `(responseAddress as slice).preloadUint(2)`.
- `onInternalMessage`: the ternary
  `var includedAddress: cell = isIncludeAddress ? beginCell()...endCell() : null;`
  fails two ways at once — `isIncludeAddress` is `int` used as a boolean
  (fixed to `!= 0`), and the ternary's `null` branch makes the whole
  expression `cell?`, not `cell` — retyped the variable as `cell?`.

Compile confirmation (`npx @ton/tolk-js jetton-minter-discoverable.tolk`, run
from `corpus/clean/converted/jetton-minter-discoverable/`):
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `jetton-minter/`

Original: [`ft/jetton-minter.fc`](https://github.com/ton-blockchain/token-contract/blob/main/ft/jetton-minter.fc)
(older/simpler variant than `jetton-minter-discoverable.fc`; still a distinct,
valid, deployable contract worth its own corpus entry).

Same three entrypoint-level fixes as `jetton-minter-discoverable/` above
(`loadData()` return type to `(int, address, cell, cell)`; `get_jetton_data()`
return cast `adminAddress as slice`; `responseAddress.preloadUint(2)` cast to
`(responseAddress as slice).preloadUint(2)`) — this file doesn't have the
`isIncludeAddress` ternary (no wallet-address-discovery logic in this older
variant).

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `jetton-minter-ico/`

Original: [`ft/jetton-minter-ICO.fc`](https://github.com/ton-blockchain/token-contract/blob/main/ft/jetton-minter-ICO.fc)
(minter variant that accepts direct Toncoin payments to mint jettons).

Same `loadData()`/`get_jetton_data()`/`preloadUint` fixes as `jetton-minter/`
above.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `jetton-wallet/`

Original: [`ft/jetton-wallet.fc`](https://github.com/ton-blockchain/token-contract/blob/main/ft/jetton-wallet.fc).

Fixes beyond the recurring patterns:
- `loadData()`'s declared return type `(int, slice, slice, cell)` (both
  `ownerAddress`/`jettonMasterAddress` loaded via `.loadAddress()`) changed to
  `(int, address, address, cell)`; `get_wallet_data()`'s declared return type
  stays `(int, slice, slice, cell)` (TEP-74 ABI), so its body was rewritten to
  destructure `loadData()` and cast both addresses:
  `(balance, ownerAddress as slice, jettonMasterAddress as slice, jettonWalletCode)`.
- `receiveTokens()`: `responseAddress.preloadUint(2)` → cast to slice first,
  same pattern as the minters.
- `sendTokens()`: `var customPayload: cell = inMsgBody.loadDict();` — `.loadDict()`
  returns `dict` (a `map`/dictionary handle), not `cell`; the variable is
  never actually used afterwards (original FunC comment even says "ignore
  custom payload"), so dropped the explicit `: cell` annotation and let it
  infer as `dict`.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

---

## `nft/` — NFT reference contracts (TEP-62)

Source: [`ton-blockchain/token-contract`](https://github.com/ton-blockchain/token-contract),
`nft/` directory, `main` branch, fetched 2026-07-18.
Command (run once from `nft/`):

```
npx --yes @ton/convert-func-to-tolk --warnings-as-comments .
```

`nft-item`/`nft-collection`/`nft-collection-editable` vendor their own copy of
the (fixed) shared siblings `op-codes.tolk` and `params.tolk` (no changes
needed beyond `params.tolk`'s trivial `workchain()`/`forceChain()` — these two
files convert clean as-is). `nft-sale` only needs `op-codes.tolk`.
`nft-marketplace` is fully self-contained (only inline op-code literals).

### `nft-item/`

Original: [`nft/nft-item.fc`](https://github.com/ton-blockchain/token-contract/blob/main/nft/nft-item.fc).

Fixes beyond the recurring patterns — this file's storage can be
"not yet initialized" (owner/content genuinely absent), which needed real
nullable-type handling, not just casts:
- `loadData()`'s return type `(int, int, slice, slice, cell)` became
  `(int, int, address, address?, cell?)` — the not-yet-initialized branch
  literally returns `(0, index, collectionAddress, null, null)`, which Tolk's
  strict null-checking requires to be reflected in the type.
- `sendMsg()`'s `payload: builder` parameter is compared against `null` at
  the call site for the "no forward payload" case; a non-nullable `builder`
  can never be null (Tolk even warns the original condition is always
  false), so retyped the parameter `builder?` and rewrote
  `if (~ (payload == null)) { msg.storeBuilder(payload); }` to
  `if (payload != null) { msg.storeBuilder(payload!); }`.
- `transferOwnership`/`onInternalMessage`: everywhere `ownerAddress`/`content`
  from `loadData()` are used after the not-yet-initialized branch has already
  returned, they're statically known non-null but still typed `address?`/
  `cell?` — used the `!` bypass-null-check operator at those call sites
  (`ownerAddress!`, `content!`), matching FunC's own lack of null-tracking
  there.
- `get_nft_data()`'s declared return type keeps non-nullable `slice`/`cell`
  (TEP-62 ABI) even though the not-yet-initialized case genuinely returns
  null at the TVM level in the original FunC too (FunC's get-methods don't
  enforce their declared types at runtime) — used `!` again to preserve that
  *exact* original (slightly unsafe) runtime behavior rather than inventing
  new placeholder values.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `nft-collection/`

Original: [`nft/nft-collection.fc`](https://github.com/ton-blockchain/token-contract/blob/main/nft/nft-collection.fc).
The converter already added `import "@stdlib/tvm-dicts"` itself (needed for
`uDictDeleteFirstAndGet`, used in batch-deploy).

Fixes beyond the recurring patterns:
- `loadData()` return type `(slice, int, cell, cell, cell)` →
  `(address, int, cell, cell, cell)`; `calculateNftItemAddress()` needed
  `as address` appended to its `beginParse()` return, same pattern as `ft/`.
- The batch-deploy `do { ... } while (!(~isF))` loop: Tolk's `do-while`
  condition could not see `isF`, a variable declared with `var` inside the
  loop body (`error: undefined symbol isF` pointing at the *condition*, not
  the body) — unlike FunC, where the `until` condition shares the body's
  scope. Rewrote as a `while (isF) { ... }` loop with `isF` predeclared
  `true` outside the loop and reassigned from the dict-deletion result
  inside it (semantically identical: both run the body at least once and
  stop on the same falsy signal).
- `deployList: cell = inMsgBody.loadRef();` — `.uDictDeleteFirstAndGet()` is
  a `dict`-typed mutating method, not a `cell` one; retyped as
  `dict` with `as dict` on the initializer.
- `uDictDeleteFirstAndGet()`'s `item` result is `slice?` (nullable) — bound it
  to a local `slice` via `!` before calling `.loadCoins()`/`.loadRef()` on it
  (calling a mutating method directly on a `foo!` temporary is itself
  rejected — `error: can not mutate a temporary expression` — so it has to be
  assigned to a named variable first).
- `royalty_params()`'s declared return type keeps `slice` for the address
  field (ABI); added `as slice` at the `rs.loadAddress()` return site.
  Same `as slice` cast applied to `get_collection_data()`'s `ownerAddress`.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `nft-collection-editable/`

Original: [`nft/nft-collection-editable.fc`](https://github.com/ton-blockchain/token-contract/blob/main/nft/nft-collection-editable.fc)
(adds a "change content" admin op on top of `nft-collection.fc`).

Identical fix set to `nft-collection/` above (same `loadData`/dict-deletion
do-while/`royalty_params`/`get_collection_data` issues — this file is a
near-verbatim superset).

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `nft-sale/`

Original: [`nft/nft-sale.fc`](https://github.com/ton-blockchain/token-contract/blob/main/nft/nft-sale.fc)
(fixed-price NFT sale/escrow contract).

Fixes beyond the recurring patterns:
- `loadData()` return type `(slice, slice, slice, int, cell)` →
  `(address, address, address, int, cell)`; `loadFees()` return type
  `(int, slice, int)` → `(int, address, int)` (both load via `.loadAddress()`).
- `var isInitialized = nftOwnerAddress.remainingBitsCount() > 2;` —
  `.remainingBitsCount()` is `slice`-only — cast to
  `(nftOwnerAddress as slice).remainingBitsCount()`; the guard was
  `if (~ isInitialized)` (bitwise-not on what's actually a `bool` from `>`),
  changed to `if (!isInitialized)`.
- `get_sale_data()`'s declared return type keeps `slice` for the three
  address fields and the royalty address (ABI) — added `as slice` at each of
  the four return-site expressions.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `nft-marketplace/`

Original: [`nft/nft-marketplace.fc`](https://github.com/ton-blockchain/token-contract/blob/main/nft/nft-marketplace.fc)
(small proxy contract that deploys per-sale/auction instances). Fully
self-contained — no sibling files.

Fixes beyond the recurring patterns:
- `loadData()`'s declared return type `slice` → `address` (loads via
  `.loadAddress()`).
- `destAddress: address = beginCell()...endCell().beginParse();` needed
  `as address` appended (`.beginParse()` yields `slice`).

This one turned out easier than expected going in — no dict/nullable
complications like the collection contracts.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

---

## `wallet-contract` — wallet-v4 and its subscription plugin

Source: [`ton-blockchain/wallet-contract`](https://github.com/ton-blockchain/wallet-contract),
`func/` directory, `main` branch, fetched 2026-07-18. `func/stdlib.fc` (FunC's
own low-level stdlib) is **not** vendored: Tolk has these primitives built
into the language/its own `@stdlib/*` modules, and the converter already
rewrites calls to use them (e.g. `@stdlib/tvm-lowlevel`, `@stdlib/tvm-dicts`,
`@stdlib/lisp-lists`, `@stdlib/gas-payments` — auto-inserted). Command (run
once from `func/`, converts `wallet-v4-code.fc` and
`simple-subscription-plugin.fc` together; `stdlib.fc` was also converted as a
side effect but unused/discarded):

```
npx --yes @ton/convert-func-to-tolk --warnings-as-comments .
```

### `wallet-v4/`

Original: [`func/wallet-v4-code.fc`](https://github.com/ton-blockchain/wallet-contract/blob/main/func/wallet-v4-code.fc).
Fully self-contained (only `@stdlib/*` imports).

This was **the trickiest file in the whole batch** — the converter left
several `_WARNING_` comments marking things it explicitly couldn't handle,
and each pointed at a genuinely different kind of problem, not just the
recurring patterns above:

- Removed the stray `tolk 1.0` version-pragma line the converter emitted
  (harmless but produces a "your Tolk compiler is newer" warning; every other
  file in this corpus omits it too).
- The plugin dictionary helpers (`isDictGet`/`isDictAddBuilder`/`isDictDelete`)
  were converted as **plain functions** taking a `dict: cell` first parameter,
  but every call site in the file uses method-call syntax
  (`plugins.isDictGet(...)`) — Tolk requires a function be declared as an
  extension method (`fun dict.name(self, ...)`) for dot-call syntax to work
  at all (`error: method isDictGet not found, but there is a global function
  named isDictGet ... should be called foo(arg), not arg.foo()`). Rewrote all
  three declarations as `fun dict.isDictGet(self, ...)` /
  `fun dict.isDictAddBuilder(self, ...)` / `fun dict.isDictDelete(self, ...)`,
  renaming their `asm(...)` operand list from `dict` to `self` to match.
- The converter emitted `var (plugins redef, isSuccess: int) = plugins.isDict...`
  three times, trying to express FunC's implicit "assign back into the same
  outer variable" pattern (`~` on a dict variable). Tolk's actual `redef`
  keyword placement (`var redef (a, b) = ...`) still didn't parse cleanly
  with the tuple-element list Tolk's grammar expected here, so sidestepped it
  entirely: each call site was rewritten to bind the new dict to a fresh
  local (e.g. `var (newPlugins1: cell, isSuccess1: int) = plugins.isDictAddBuilder(...)`)
  and then plain-assign `plugins = newPlugins1 as dict;` on the next line —
  same effect, no special syntax needed.
- Every `wcNAddress` built via `beginCell()...beginParse()` (or
  `.loadBits(8+256)`) is `address` by declared type but fed into the
  `slice`-parametered dict helpers above — needed `as slice` at each call
  site, on top of the `as address` needed at each *construction* site (both
  directions of the same address/slice split, in the same function).
- `get_plugin_list()`'s `do { ... } while (!(~f))` loop had the same
  loop-body-variable-invisible-to-the-condition issue as `nft-collection/`'s
  do-while — rewritten as a `while` loop the same way.
- Inside that same loop, the converter left
  `/* _WARNING_ function 'pair' has been removed */pair(wc, addr)` — FunC's
  `pair()` helper (bundles two stack values into a 2-tuple) has no Tolk
  equivalent function; Tolk's tuple literal syntax replaces it directly, so
  `list = listPrepend(pair(wc, addr), list);` became manual lisp-list-style
  consing: `list = [[wc, addr], list] as tuple?;` (`listPrepend` itself also
  doesn't exist as a free function — it's a method on the generic
  `lisp_list<T>` type in `@stdlib/lisp-lists`, not worth pulling in for one
  call site here).

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `simple-subscription-plugin/`

Original: [`func/simple-subscription-plugin.fc`](https://github.com/ton-blockchain/wallet-contract/blob/main/func/simple-subscription-plugin.fc).
Fully self-contained (only `@stdlib/gas-payments`).

Fixes beyond the recurring patterns:
- `loadStorage()`'s return type `(slice, slice, int, ...)` → `(address, address, int, ...)`;
  `saveStorage()`'s `wallet`/`beneficiary` parameters similarly retyped
  `address`, with `as slice` added at their `.storeSlice(...)` call sites.
- `forwardFunds(beneficiary: slice, isSelfDestruct: int, op: int)` — every
  call site passes the FunC-style flag as a literal `true`/`false`, which
  doesn't typecheck against `int`; retyped the parameter as `bool` and
  adjusted the two `int`-flavored conditionals inside the function body to
  plain `bool` usage (`if (!isSelfDestruct)`, `if (isSelfDestruct)`).
  `beneficiary` retyped `address` with `as slice` at its `.storeSlice(...)`
  use.
- `forwardFunds()`'s `.storeDict(pair_second(contract.getOriginalBalanceWithExtraCurrencies()))` —
  `pair_second()` (another removed FunC stdlib helper) was replaced by
  destructuring the tuple directly: `var [_, extraCurrencies] = contract.getOriginalBalanceWithExtraCurrencies();`.
  `extraCurrencies` then turned out to be Tolk's high-level `ExtraCurrenciesMap`
  (`map<int32, varuint32>`), not the low-level `dict` that `.storeDict()`
  expects — `error: type ExtraCurrenciesMap can not be cast to dict` on a
  plain `as dict` attempt — resolved with the map type's own
  `.toLowLevelDict()` conversion method instead of a cast.
- `requestPayment()`/`selfDestruct()`: same `slice`→`address` parameter
  retyping and `as slice` at `.storeSlice()` call sites as `saveStorage()`
  above.
- `isSliceDataEqual(s1: slice, s2: slice)` is called with `address`-typed
  arguments (`sAddr`, `beneficiary`, `wallet`) — added `as slice` at each of
  the three call sites; its `int`-returning result is used directly in
  `if (...)` (`!= 0`) and once through `~` (`isSliceDataEqual(...) == 0`).
- `shortMsgFwdFee()`/`gasToCoins()`: `blockchain.configParam(configIndex)` is
  `cell?` (a chain config parameter may not exist), but the converter called
  `.beginParse()` straight on it — added `!` (`configParam(...)!.beginParse()`)
  to assert non-null, matching FunC's original lack of a null-check there.
- `get_subscription_data()`'s removed-function
  `pair(wallet.getWorkchainAndHash())` (same `pair()` issue as `wallet-v4/`)
  rewritten by destructuring `getWorkchainAndHash()`'s result into named
  locals first, then building a `[wc, hash]` tuple literal at the return site.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

---

## Classic reference wallets

Source: fetched directly (not cloned) from
`https://raw.githubusercontent.com/ton-blockchain/ton/master/crypto/smartcont/`,
2026-07-18. Command (run once from the scratch dir containing all three
fetched `.fc` files):

```
npx --yes @ton/convert-func-to-tolk --warnings-as-comments .
```

### `wallet3/`

Original: [`crypto/smartcont/wallet3-code.fc`](https://raw.githubusercontent.com/ton-blockchain/ton/master/crypto/smartcont/wallet3-code.fc)
(the classic simple wallet v3 — no `#include`, fully self-contained, only
`@stdlib/*` imports needed).

Only fix needed, beyond the recurring int-as-bool pattern:
- `while (cs.slice_refs())` → `while (cs.remainingRefsCount() != 0)`.

This was the simplest file in the whole batch — its small size and lack of
addresses/dicts meant almost none of the recurring patterns even applied.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

### `simple-wallet/`

Original: [`crypto/smartcont/simple-wallet-code.fc`](https://raw.githubusercontent.com/ton-blockchain/ton/master/crypto/smartcont/simple-wallet-code.fc)
(an even more minimal wallet, no seqno-in-a-loop send batching guard beyond a
single message). Self-contained.

Fixes:
- `if (cs.remainingRefsCount())` → `if (cs.remainingRefsCount() != 0)`.
- The converter emitted `var cs2 = beginParse(contract.getData());` — a
  free-function call syntax that doesn't exist in Tolk (`beginParse` is a
  method, not a global function: `error: undefined symbol beginParse`) —
  rewritten as `contract.getData().beginParse()`.

Compile confirmation:
```
Compiling using Tolk v1.4.2
Compiled successfully!
```

---

## Dropped candidates

- **`highload-wallet-v2-code.fc`** (from the same
  `crypto/smartcont/` directory as `wallet3`/`simple-wallet` above, fetched
  the same way). Converted, but not fixed up or vendored — this is a genuine
  semantic mismatch between FunC's and Tolk's dictionary APIs, not just a
  type-annotation gap, and fixing it correctly would mean re-implementing
  dict deletion at the `asm` level rather than a mechanical fix:
  the original FunC expiry-cleanup loop calls
  `old_queries.udict_delete_get_min(64)` **non-mutating** — it returns a
  *new* dict as its first result, and the code only commits to that new dict
  (`old_queries = old_queries';`) if the deleted record turns out to actually
  be expired (`i < bound`); otherwise the original `old_queries` is left
  untouched and the loop stops. Tolk's stdlib equivalent,
  `dict.uDictDeleteFirstAndGet(mutate self, keyLen)`, mutates its receiver
  **unconditionally** as part of the call — there's no non-mutating variant
  in `@stdlib/tvm-dicts`. Calling it directly would delete the record from
  `old_queries` even on the branch that's supposed to leave it alone,
  silently changing contract behavior (the resulting `get_plugin_list`-style
  history of tracked queries would diverge from the original algorithm).
  The converter's own output also left three unresolved `todo` placeholder
  types and a `redef`-inside-tuple-destructure pattern in the same function,
  compounding the effort. Judged not worth the risk of vendoring a
  behaviorally-wrong "clean" fixture into an accuracy-benchmark corpus, and
  dropped after the ~10-minute budget — 13 other candidates already met the
  ≥13 target with margin to spare.
