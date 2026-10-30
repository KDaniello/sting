// Hand-authored, not vendored — see corpus/fixtures/bridge/SOURCE.md.
// Minimal ID (Inconsistent Data) pattern, dispatched by opcode only — see
// UbmMintPattern.tolk's header comment for why.

const OP_SAVE_FIELDS = 0x53415645; // "SAVE"

fun saveFields(a: int, b: int) {
    var builder = beginCell().storeUint(a, 32).storeUint(b, 32);
    contract.setData(builder.endCell());
}

fun loadFields() {
    var s = contract.getData().beginParse();
    // BUG (ID class, docs/blocks/02-detectors-v1.md): `saveFields` writes
    // (a, b) in that order, but this reads them back into (b, a) — the two
    // local names are swapped relative to the storage write order.
    val b = s.loadUint(32);
    val a = s.loadUint(32);
    s.assertEnd();
    return (a, b);
}

fun onInternalMessage(in: InMessage) {
    val op = in.body.loadUint(32);
    if (op == OP_SAVE_FIELDS) {
        val a = in.body.loadUint(32);
        val b = in.body.loadUint(32);
        saveFields(a, b);
    }
}

get fun getFields(): (int, int) {
    return loadFields();
}
