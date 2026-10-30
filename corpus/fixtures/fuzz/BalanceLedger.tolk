// Hand-authored, not vendored — see corpus/fixtures/fuzz/SOURCE.md for why.
// A deliberately vulnerable contract, purpose-built to validate Block 06's
// fuzzing harness (docs/blocks/06-fuzzing-harness.md §6.3: "формальная
// компиляция и 'не упал за 5 минут' — недостаточное доказательство
// работоспособности фаззера"). Reachable through a single crafted message
// via `TvmEmulator::send_internal_message` (a message body + attached
// value, docs/blocks/04-emulator-ffi.md) — deliberately NOT modeled on
// Block 02/03's UBM pattern (missing `onBouncedMessage`), which needs a
// full protocol-level bounced-message envelope
// (`CommonMsgInfo.bounced`) that this narrower emulator API cannot deliver
// at all; see docs/blocks/06-fuzzing-harness.md's Результаты for the full
// reasoning behind picking a differently-reachable bug class instead.

const OP_CREDIT = 1;
const OP_DEBIT = 2;

struct Storage {
    balance: int
}

fun Storage.load() {
    var s = contract.getData().beginParse();
    val balance = s.loadInt(64);
    s.assertEnd();
    return Storage { balance };
}

fun Storage.save(self) {
    contract.setData(beginCell().storeInt(self.balance, 64).endCell());
}

fun onInternalMessage(in: InMessage) {
    val op = in.body.loadUint(32);
    val amount = in.body.loadUint(64);
    var storage = Storage.load();
    if (op == OP_CREDIT) {
        storage.balance += amount;
    } else if (op == OP_DEBIT) {
        // BUG (deliberate, CWE-191 class): no check that
        // `storage.balance >= amount` before debiting — a single message
        // with op=DEBIT and any amount > 0 drives `balance` negative with
        // no throw, no revert, `success: true` — a silent invariant
        // violation, not a crash. This is exactly the shape of bug this
        // block's harness must demonstrably catch.
        storage.balance -= amount;
    }
    storage.save();
}

get fun getBalance(): int {
    val storage = Storage.load();
    return storage.balance;
}
