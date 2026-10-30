fun save(balance: int, owner: int) {
    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);
    contract.setData(b.endCell());
}

fun load() {
    var s = contract.getData().beginParse();
    val owner = s.loadUint(256);
    val balance = s.loadUint(64);
}
