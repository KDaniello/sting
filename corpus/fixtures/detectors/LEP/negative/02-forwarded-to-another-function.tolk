fun handleMessage(s: slice) {
    return parseElsewhere(s);
}

fun parseElsewhere(s: slice) {
    val opcode = s.loadUint(32);
    s.assertEnd();
}
