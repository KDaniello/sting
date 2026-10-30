fun parseWithFlag(s: slice, hasExtra: bool) {
    val opcode = s.loadUint(32);
    if (hasExtra) {
        s.assertEnd();
    } else {
        // forgot to assertEnd on this path
    }
}
