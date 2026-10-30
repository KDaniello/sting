fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    val recipient: slice = inMsgBody.loadAddress();
    inMsgBody.assertEnd();
    val recipientHash: int = recipient.hash();
    logRecipientHash(recipientHash);
}
