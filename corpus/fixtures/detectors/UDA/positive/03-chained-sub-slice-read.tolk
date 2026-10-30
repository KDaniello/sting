fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    val tail: slice = inMsgBody.loadBits(32);
    inMsgBody.assertEnd();
    val recipient: slice = tail.loadAddress();
    val relayMsg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();
    sendRawMessage(relayMsg, 64);
}
