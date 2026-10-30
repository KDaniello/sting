# Source

`BalanceLedger.tolk` — hand-authored, not vendored, specifically for
Block 06 (`docs/security/blocks/06-fuzzing-harness.md`, в истории: `git show cf52463:docs/security/blocks/06-fuzzing-harness.md`)'s harness-validation
requirement (§6.3: "формальная компиляция и 'не упал за 5 минут' —
недостаточное доказательство работоспособности фаззера"). A deliberately
vulnerable contract with one seeded bug: `onInternalMessage`'s `OP_DEBIT`
branch subtracts from `Storage.balance` without checking
`balance >= amount` first (CWE-191, missing-check underflow) — reachable
through a single crafted message via
`TvmEmulator::send_internal_message` (`docs/security/blocks/04-emulator-ffi.md`, в истории: `git show cf52463:docs/security/blocks/04-emulator-ffi.md`)
(a message body + attached value, not a full protocol message envelope).

Not modeled on Block 02/03's UBM pattern (missing `onBouncedMessage`)
deliberately — that bug needs a full `CommonMsgInfo.bounced` bit delivered
through a real protocol-level `Message` envelope, which
`send_internal_message`'s narrower API (body + amount only) cannot deliver
at all (confirmed by reading `emulator-extern.cpp`, not assumed — see
Block 06's Результаты for the full reasoning). This fixture's bug class is
chosen specifically to be reachable through the emulator API surface
Block 04 already ships, not because UBM itself is a bad target in principle.

Compiled 18.07.2026 with the real `npx @ton/tolk-js` compiler (Tolk v1.4.2,
same toolchain used throughout `corpus/clean/`'s compile-verification):

```sh
npx --yes @ton/tolk-js -o BalanceLedger.output.json BalanceLedger.tolk
```

- `BalanceLedger.code.boc.base64` — the compiler's own `codeBoc64` output field.
- `BalanceLedger.data.boc.base64` — a hand-built initial data cell
  (`storeInt(0, 64)`, i.e. `balance = 0`), built with `tonlib_core::cell::CellBuilder`
  (the same crate `sting-emulator` itself uses), not hand-encoded bytes.

Verified against the real vendored `libemulator` (`vendor/libemulator/`,
same build used throughout Block 04/05's tests) before being committed as a
fixture, not just compiled: `getBalance()` on the fresh contract returns
`0`; sending `OP_CREDIT` with `amount=100` then `OP_DEBIT` with `amount=500`
(overdrawing on purpose) leaves `getBalance()` returning **`-400`**, with
`vm_exit_code: 0` and `success: true` on every call — a silent invariant
violation, not a crash or a thrown exit code. This is the exact defect
Block 06's fuzz harness is validated against.
