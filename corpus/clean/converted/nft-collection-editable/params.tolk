@pure
fun workchain(): int
    asm "0 PUSHINT";

fun forceChain(addr: address) {
  var (wc: int, _) = addr.getWorkchainAndHash();
  assert(wc == workchain()) throw 333;
}
