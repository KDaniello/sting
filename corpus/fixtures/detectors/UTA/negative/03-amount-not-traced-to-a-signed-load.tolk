fun transfer(from: slice, to: slice, amount: int) {
    var fromBalance: int = 10;
    var toBalance: int = 0;

    fromBalance -= amount;
    toBalance += amount;

    assert(fromBalance > 0, 502);

    saveBalances(fromBalance, toBalance);
}
