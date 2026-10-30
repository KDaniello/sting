fun adjustBalances(inMsgBody: slice) {
    val amount: int = inMsgBody.loadInt(256);
    var fromBalance: int = 10;
    var toBalance: int = 0;

    fromBalance -= amount;
    toBalance += amount;

    assert(toBalance > 0, 503);

    saveBalances(fromBalance, toBalance);
}
