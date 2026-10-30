fun save(amount: int, owner: int) {
    var b = beginCell().storeCoins(amount).storeUint(owner, 256);
    contract.setData(b.endCell());
}

fun load() {
    var s = contract.getData().beginParse();
    val amount = s.loadUint(64);
    val owner = s.loadUint(256);
}
