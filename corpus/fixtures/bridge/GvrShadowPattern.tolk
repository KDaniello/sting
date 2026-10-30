// Hand-authored, not vendored — see corpus/fixtures/bridge/SOURCE.md.
// Minimal GVR (Global Var Redefined) pattern, dispatched by opcode only —
// see UbmMintPattern.tolk's header comment for why (in.senderAddress is not
// reliably reachable via TvmEmulator::send_internal_message).

const OP_BUGGY_INCREMENT = 0x494e4352; // "INCR"

contract GvrShadowPattern {
    author: "sting fixture"
    storage: Storage
}

struct Storage {
    counter: int64
}

fun Storage.load() {
    var s = contract.getData().beginParse();
    val counter = s.loadInt(64);
    s.assertEnd();
    return Storage { counter };
}

fun Storage.save(self) {
    contract.setData(beginCell().storeInt(self.counter, 64).endCell());
}

fun onInternalMessage(in: InMessage) {
    val op = in.body.loadUint(32);
    if (op == OP_BUGGY_INCREMENT) {
        var storage = Storage.load();
        // BUG (GVR class, docs/blocks/02-detectors-v1.md): `counter` here is
        // a fresh local, not `storage.counter` — this line visually reads
        // like an increment but never touches storage.counter at all.
        var counter = storage.counter + 1;
        storage.save();
    }
}

get fun getCounter(): int {
    return Storage.load().counter;
}
