const OP_UNLOCK = "op::unlock".crc32();

global ctxOwner: address;
global ctxLocked: bool;

fun loadData() {
    var ds = contract.getData().beginParse();
    ctxOwner = ds.loadAddress();
    ctxLocked = ds.loadBool();
    ds.assertEnd();
}

fun saveData() {
    contract.setData(
        beginCell()
            .storeAddress(ctxOwner)
            .storeBool(ctxLocked)
        .endCell()
    );
}

fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    var cs: slice = inMsgFull.beginParse();
    cs.loadUint(4);
    val senderAddress: address = cs.loadAddress();
    loadData();
    val op: int = inMsgBody.loadUint(32);
    if (op == OP_UNLOCK) {
        assert (senderAddress == ctxOwner) throw 401;
        ctxLocked = false;
        saveData();
        return;
    }
    throw 0xffff;
}
