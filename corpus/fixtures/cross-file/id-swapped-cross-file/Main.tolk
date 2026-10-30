import "storage"

fun save(balance: int, owner: int) {
    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);
    contract.setData(b.endCell());
}

fun onInternalMessage(in: InMessage) {
    save(100, 200);
}
