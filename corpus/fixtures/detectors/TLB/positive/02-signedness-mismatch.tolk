fun save(balance: int, owner: int) {
    var b = beginCell().storeInt(balance, 64).storeUint(owner, 256);
    contract.setData(b.endCell());
}

fun load() {
    var s = contract.getData().beginParse();
    val balance = s.loadUint(64);
    val owner = s.loadUint(256);
}
