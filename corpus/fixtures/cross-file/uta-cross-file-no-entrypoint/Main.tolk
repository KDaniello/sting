import "helper"

fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    val amount: int = inMsgBody.loadInt(256);
    transfer(beginCell().endCell(), inMsgFull.beginParse(), inMsgFull.beginParse(), amount);
}
