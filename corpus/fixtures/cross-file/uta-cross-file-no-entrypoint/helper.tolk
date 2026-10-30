fun transfer(balances: cell, from: slice, to: slice, amount: int): cell {
    var fromBalance: int = 10;
    var toBalance: int = 0;
    fromBalance -= amount;
    toBalance += amount;
    assert(fromBalance > 0, 502);
    return balances;
}
