const OP_UNLOCK = "op::unlock".crc32();

global ctxPlayer: address;
global ctxLocked: bool;

fun loadData() {
    var ds = contract.getData().beginParse();
    ctxPlayer = ds.loadAddress();
    ctxLocked = ds.loadBool();
    ds.assertEnd();
}

fun saveData() {
    contract.setData(
        beginCell()
            .storeAddress(ctxPlayer)
            .storeBool(ctxLocked)
        .endCell()
    );
}

fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    loadData();
    val op: int = inMsgBody.loadUint(32);
    if (op == OP_UNLOCK) {
        ctxLocked = false;
        saveData();
        return;
    }
    throw 0xffff;
}
