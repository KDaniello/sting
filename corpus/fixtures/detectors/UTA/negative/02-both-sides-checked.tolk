fun adjustBalances(inMsgBody: slice) {
    val amount: int = inMsgBody.loadInt(256);
    var fromBalance: int = 10;
    var toBalance: int = 0;

    fromBalance -= amount;
    toBalance += amount;

    assert(fromBalance > 0, 502);
    assert(toBalance > 0, 503);

    saveBalances(fromBalance, toBalance);
}
