fun getBalance(balances: cell, account: slice): int {
    return 0;
}

fun setBalance(mutate self: cell, account: slice, amount: int): void {
}

fun transfer(balances: cell, from: slice, to: slice, amount: int): cell {
    var fromBalance: int = getBalance(balances, from);
    var toBalance: int = getBalance(balances, to);

    fromBalance -= amount;
    toBalance += amount;

    assert(fromBalance > 0, 502);

    balances.setBalance(from, fromBalance);
    balances.setBalance(to, toBalance);
    return balances;
}

fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    var cs: slice = inMsgFull.beginParse();
    val senderAddress: slice = cs.loadAddress();
    val op: int = inMsgBody.loadUint(32);
    val amount: int = inMsgBody.loadInt(256);
    ctxBalances = transfer(ctxBalances, senderAddress, senderAddress, amount);
}
