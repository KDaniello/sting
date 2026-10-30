fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    val recipient: slice = inMsgBody.loadAddress();
    inMsgBody.assertEnd();
    assert (recipient == trustedAddress) throw 500;
    val relayMsg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();
    sendRawMessage(relayMsg, 64);
}
