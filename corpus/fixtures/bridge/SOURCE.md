# Source

`UbmMintPattern.tolk` — hand-authored, not vendored, for
Block 07 (`git show a7280d2:docs/security/blocks/07-static-dynamic-bridge.md`, в истории: `git show cf52463:git show a7280d2:docs/security/blocks/07-static-dynamic-bridge.md`)'s required
end-to-end demonstration (§7.4: "минимум один класс дефекта воспроизведён
end-to-end"). Same semantic pattern as
`corpus/fixtures/detectors/UBM/positive/03-real-jetton-mint-pattern.tolk`
(itself a direct isolated copy of the real
`ton-blockchain/tolk-bench` `01_jetton/JettonMinter.tolk` mint chunk,
Block 02 (`git show a7280d2:docs/security/blocks/02-detectors-v1.md`, в истории: `git show cf52463:git show a7280d2:docs/security/blocks/02-detectors-v1.md`)) — storage committed via
`.save()`, then a bounceable message sent, with no `onBouncedMessage`
handler defined anywhere in the file.

**One deliberate difference from the real `JettonMinter.tolk`**: dispatch is
by opcode only, not additionally gated behind
`in.senderAddress == storage.adminAddress`. This is not a simplification for
convenience — it is a confirmed, necessary workaround. During this block's
implementation, direct experimentation (compiling a scratch copy of the real
`JettonMinter.tolk` with debug get-methods, run against the real vendored
`libemulator`) established that `INMSG_SRC`/`INMSG_BOUNCED` do not populate
correctly through `TvmEmulator::send_internal_message`'s execution path: it
calls `SmartContract::send_internal_message`, which invokes `recv_internal`
directly via a 4-value stack (`balance`, `msg_value`, `in_msg_full`,
`in_msg_body`) — a genuine, direct method call, not a full
transaction/collator context. `INMSG_SRC`'s attempt to read the parsed
sender address in that reduced context threw `not a MsgAddressInt` in a
real run, even though the same `in_msg_full` cell (confirmed byte-for-byte
against the real Tolk compiler's own struct serialization) hardcodes the
sender exactly as `SmartContract.cpp`'s `build_internal_message` documents.
Any finding whose triggering scenario needs `in.senderAddress` (or bounce
delivery — see `corpus/fixtures/fuzz/SOURCE.md`'s related Block 06 finding)
is not dynamically reachable through this API, independent of how carefully
the caller's message is built. See
`git show a7280d2:docs/security/blocks/07-static-dynamic-bridge.md`'s Результаты for the full
investigation and its consequences for `BridgeController`'s UBM strategy.

Compiled 18.07.2026 with the real `npx @ton/tolk-js` compiler (Tolk v1.4.2):

```sh
npx --yes @ton/tolk-js -o UbmMintPattern.output.json UbmMintPattern.tolk
```

- `UbmMintPattern.code.boc.base64` — the compiler's own `codeBoc64` field.
- `UbmMintPattern.data.boc.base64` — a hand-built initial data cell
  (`storeCoins(0)`, i.e. `totalSupply = 0`), built with
  `tonlib_core::cell::CellBuilder` (the same crate `sting-emulator` itself
  uses), not hand-encoded bytes.

Verified against the real vendored `libemulator` before being committed:
sending a `MINT`-opcode message with `amount = 1_000_000_000` returns
`success: true`, `accepted: true`, `vm_exit_code: 0`; `getTotalSupply()`
immediately after returns `1_000_000_000` (persisted, not reverted); the
response's `actions` field contains the queued bounceable outbound message.
Both confirmed by `sting`'s own static `UBM` detector (flags the file) and
this dynamic run (confirms the unrevertable commit-then-send is real) —
exactly the two halves `BridgeController::verify_ubm` checks.
