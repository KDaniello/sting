fun relayTo(dest: slice) {
    val relayMsg: cell = beginCell().storeUint(0x18, 6).storeSlice(dest).storeCoins(0).endCell();
    sendRawMessage(relayMsg, 64);
}

fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    val recipient: slice = inMsgBody.loadAddress();
    inMsgBody.assertEnd();
    relayTo(recipient);
}
