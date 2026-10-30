fun load() {
    var s = contract.getData().beginParse();
    val owner = s.loadUint(256);
    val balance = s.loadUint(64);
}
