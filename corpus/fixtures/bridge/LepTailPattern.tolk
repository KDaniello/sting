// Hand-authored, not vendored — see corpus/fixtures/bridge/SOURCE.md.
// Minimal LEP (Lack End Parse) pattern, dispatched by opcode only — see
// UbmMintPattern.tolk's header comment for why.

const OP_SET_VALUE = 0x53455456; // "SETV"

struct Storage {
    value: int
}

fun Storage.load() {
    var s = contract.getData().beginParse();
    val value = s.loadInt(64);
    s.assertEnd();
    return Storage { value };
}

fun Storage.save(self) {
    contract.setData(beginCell().storeInt(self.value, 64).endCell());
}

// BUG (LEP class, docs/blocks/02-detectors-v1.md): `body` is a `slice`
// *parameter* read from and never `assertEnd()`-ed — LEP v1 only tracks
// slice-typed parameters, not field accesses like `in.body` directly (see
// lep.rs's own module doc comment), so the vulnerable read is factored into
// this helper specifically to stay within that documented, verified scope.
fun parseSetValue(body: slice): int {
    return body.loadUint(32);
}

fun onInternalMessage(in: InMessage) {
    val op = in.body.loadUint(32);
    if (op == OP_SET_VALUE) {
        val value = parseSetValue(in.body);
        var storage = Storage.load();
        storage.value = value;
        storage.save();
    }
}

get fun getValue(): int {
    return Storage.load().value;
}
