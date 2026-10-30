fun saveTriple(a: int, b: int, c: int) {
    var cell = beginCell().storeUint(a, 32).storeUint(b, 32).storeUint(c, 32);
    contract.setData(cell.endCell());
}

fun loadTriple() {
    var s = contract.getData().beginParse();
    val c = s.loadUint(32);
    val a = s.loadUint(32);
    val b = s.loadUint(32);
}
