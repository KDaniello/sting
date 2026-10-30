fun adjustBalances(inMsgBody: slice) {
    val amount: int = inMsgBody.loadInt(256);
    var fromBalance: int = 10;
    var toBalance: int = 0;

    fromBalance -= amount;
    toBalance += amount;

    saveBalances(fromBalance, toBalance);
}
