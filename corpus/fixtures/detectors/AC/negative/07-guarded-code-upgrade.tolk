const OP_UPGRADE = "op::upgrade".crc32();

global ctxOwner: address;

fun loadData() {
    var ds = contract.getData().beginParse();
    ctxOwner = ds.loadAddress();
    ds.assertEnd();
}

fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    var cs: slice = inMsgFull.beginParse();
    cs.loadUint(4);
    val senderAddress: address = cs.loadAddress();
    loadData();
    val op: int = inMsgBody.loadUint(32);
    if (op == OP_UPGRADE) {
        assert (senderAddress == ctxOwner) throw 401;
        val code: cell = inMsgBody.loadRef();
        contract.setCodePostponed(code);
        return;
    }
    throw 0xffff;
}
