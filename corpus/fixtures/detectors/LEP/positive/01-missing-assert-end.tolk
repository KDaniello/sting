fun parseIncoming(s: slice) {
    val opcode = s.loadUint(32);
    assert (opcode == 1) throw 100;
}
