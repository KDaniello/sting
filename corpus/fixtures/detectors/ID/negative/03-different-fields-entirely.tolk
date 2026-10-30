fun save(balance: int, owner: int) {
    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);
    contract.setData(b.endCell());
}

fun load() {
    var s = contract.getData().beginParse();
    val ownerAddressHash = s.loadUint(256);
    val extraField = s.loadUint(64);
}
