// NFT marketplace smart contract

// storage scheme
// storage#_ owner_address:MsgAddress
//           = Storage;

@inline
fun loadData(): address {
  var ds = contract.getData().beginParse();
  return 
    (ds.loadAddress() // owner
     );
}

@inline
fun saveData(ownerAddress: address) {
  contract.setData(beginCell()
    .storeAddress(ownerAddress)
    .endCell());
}

// in the future, use: fun onInternalMessage(in: InMessage) {
fun onInternalMessage(msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    if (inMsgBody.isEmpty()) { // ignore empty messages
        return;
    }
    var cs: slice = inMsgFull.beginParse();
    var flags: int = cs.loadUint(4);

    if ((flags & 1) != 0) {  // ignore all bounced messages
        return;
    }
    var senderAddress: address = cs.loadAddress();

    var (ownerAddress) = loadData();
    assert(senderAddress.bitsEqual(ownerAddress)) throw 401;
    var op: int = inMsgBody.loadUint(32);
  
    if (op == 1) { // deploy new auction
      var amount: int = inMsgBody.loadCoins();
      var (stateInit: cell, body: cell) = (cs.loadRef(), cs.loadRef());
      var stateInitHash: int = stateInit.hash();
      var destAddress: address = beginCell().storeInt(0, 8).storeUint(stateInitHash, 256).endCell().beginParse() as address;
       
      var msg = beginCell()
        .storeUint(0x18, 6)
        .storeUint(4, 3).storeAddress(destAddress)
        .storeCoins(amount)
        .storeUint(4 + 2 + 1, 1 + 4 + 4 + 64 + 32 + 1 + 1 + 1)
        .storeRef(stateInit)
        .storeRef(body);
      sendRawMessage(msg.endCell(), 1); // paying fees, revert on errors
    }
}

fun onExternalMessage(inMsg: slice) {
}
