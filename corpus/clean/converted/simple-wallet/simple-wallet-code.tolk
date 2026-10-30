import "@stdlib/gas-payments"
import "@stdlib/tvm-lowlevel"
// Simple wallet smart contract

// in the future, use: fun onInternalMessage(in: InMessage) {
fun onInternalMessage(inMsg: slice) {
  // do nothing for internal messages
}

fun onExternalMessage(inMsg: slice) {
  var signature = inMsg.loadBits(512);
  var cs = inMsg;
  var msgSeqno: int = cs.loadUint(32);
  var cs2 = contract.getData().beginParse();
  var storedSeqno = cs2.loadUint(32);
  var publicKey = cs2.loadUint(256);
  cs2.assertEnd();
  assert(msgSeqno == storedSeqno) throw 33;
  assert(isSignatureValid(inMsg.hash(), signature, publicKey)) throw 34;
  acceptExternalMessage();
  cs.stackMoveToTop();
  if (cs.remainingRefsCount() != 0) {
    var mode = cs.loadUint(8);
    sendRawMessage(cs.loadRef(), mode);
  }
  cs.assertEnd();
  contract.setData(beginCell().storeUint(storedSeqno + 1, 32).storeUint(publicKey, 256).endCell());
}
