fun parseIncoming(s: slice) {
    val opcode = s.loadUint(32);
    s.assertEnd();
}
