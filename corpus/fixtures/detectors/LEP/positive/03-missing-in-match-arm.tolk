fun parseByOpcode(s: slice, opcode: int) {
    match (opcode) {
        1 => {
            val flag = s.loadUint(1);
            s.assertEnd();
        }
        else => {
            // this arm reads a field but never validates s is fully consumed
            val extra = s.loadUint(32);
        }
    }
}
