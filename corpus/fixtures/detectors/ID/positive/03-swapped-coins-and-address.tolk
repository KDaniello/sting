fun saveAccount(balance: int, owner: int) {
    var cell = beginCell().storeCoins(balance).storeAddress(owner);
    contract.setData(cell.endCell());
}

fun loadAccount() {
    var s = contract.getData().beginParse();
    val owner = s.loadAddress();
    val balance = s.loadCoins();
}
