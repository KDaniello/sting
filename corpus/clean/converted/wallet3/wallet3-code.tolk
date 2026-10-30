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
  var (subwalletId, validUntil, msgSeqno) = (cs.loadUint(32), cs.loadUint(32), cs.loadUint(32));
  assert(!(validUntil <= blockchain.now())) throw 35;
  var ds = contract.getData().beginParse();
  var (storedSeqno, storedSubwallet, publicKey) = (ds.loadUint(32), ds.loadUint(32), ds.loadUint(256));
  ds.assertEnd();
  assert(msgSeqno == storedSeqno) throw 33;
  assert(subwalletId == storedSubwallet) throw 34;
  assert(isSignatureValid(inMsg.hash(), signature, publicKey)) throw 35;
  acceptExternalMessage();
  cs.stackMoveToTop();
  while (cs.remainingRefsCount() != 0) {
    var mode = cs.loadUint(8);
    sendRawMessage(cs.loadRef(), mode);
  }
  contract.setData(beginCell()
    .storeUint(storedSeqno + 1, 32)
    .storeUint(storedSubwallet, 32)
    .storeUint(publicKey, 256)
    .endCell());
}

// Get methods

get fun seqno(): int {
  return contract.getData().beginParse().preloadUint(32);
}

get fun get_public_key(): int {
  var cs = contract.getData().beginParse();
  cs.loadUint(64);
  return cs.preloadUint(256);
}
