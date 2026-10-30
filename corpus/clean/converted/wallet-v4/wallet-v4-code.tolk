import "@stdlib/gas-payments"
import "@stdlib/tvm-lowlevel"
import "@stdlib/tvm-dicts"
import "@stdlib/lisp-lists"

// Wallet smart contract with plugins

@pure
fun dict.isDictGet(self, keyLen: int, index: slice): (slice, int)
    asm(index self keyLen) "DICTGET" "NULLSWAPIFNOT";
@pure
fun dict.isDictAddBuilder(self, keyLen: int, index: slice, value: builder): (cell, int)
    asm(value index self keyLen) "DICTADDB";
@pure
fun dict.isDictDelete(self, keyLen: int, index: slice): (cell, int)
    asm(index self keyLen) "DICTDEL";

// in the future, use: fun onInternalMessage(in: InMessage) {
fun onInternalMessage(msgValue: int, inMsgCell: cell, inMsg: slice) {
  var cs = inMsgCell.beginParse();
  var flags = cs.loadUint(4);  // int_msg_info$0 ihr_disabled:Bool bounce:Bool bounced:Bool
  if ((flags & 1) != 0) {
    // ignore all bounced messages
    return;
  }
  if (inMsg.remainingBitsCount() < 32) {
    // ignore simple transfers
    return;
  }
  var op: int = inMsg.loadUint(32);
  if ((op != 0x706c7567) & (op != 0x64737472)) { // "plug" & "dstr"
    // ignore all messages not related to plugins
    return;
  }
  var sAddr: address = cs.loadAddress();
  var (wc: int, addrHash: int) = sAddr.getWorkchainAndHash();
  var wcNAddress: address = beginCell().storeInt(wc, 8).storeUint(addrHash, 256).endCell().beginParse() as address;
  var ds = contract.getData().beginParse()/* _WARNING_ method .skipBits() now mutates the object and returns self */.skipBits(32 + 32 + 256);
  var plugins = ds.loadDict();
  var (_, isSuccess) = plugins.isDictGet(8 + 256, wcNAddress as slice);
  if (isSuccess == 0) {
    // it may be a transfer
    return;
  }
  var queryId: int = inMsg.loadUint(64);
  var msg = beginCell();
  if (op == 0x706c7567) { // request funds

    var (rToncoins: int, rExtra: dict) = (inMsg.loadCoins(), inMsg.loadDict());

    var [myBalance: int, _] = contract.getOriginalBalanceWithExtraCurrencies();
    assert(myBalance - msgValue >= rToncoins) throw 80;

    msg = msg.storeUint(0x18, 6)
             .storeAddress(sAddr)
             .storeCoins(rToncoins)
             .storeDict(rExtra)
             .storeUint(0, 4 + 4 + 64 + 32 + 1 + 1)
             .storeUint(0x706c7567 | 0x80000000, 32)
             .storeUint(queryId, 64);
    sendRawMessage(msg.endCell(), 64);

  }

  if (op == 0x64737472) { // remove plugin by its request

    plugins.isDictDelete(8 + 256, wcNAddress as slice);
    var ds = contract.getData().beginParse().getFirstBits(32 + 32 + 256);
    contract.setData(beginCell().storeSlice(ds).storeDict(plugins).endCell());
    // return coins only if bounce expected
    if ((flags & 2) != 0) {
      msg = msg.storeUint(0x18, 6)
               .storeAddress(sAddr)
               .storeCoins(0)
               .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
               .storeUint(0x64737472 | 0x80000000, 32)
               .storeUint(queryId, 64);
      sendRawMessage(msg.endCell(), 64);
    }
  }
}

fun onExternalMessage(inMsg: slice) {
  var signature = inMsg.loadBits(512);
  var cs = inMsg;
  var (subwalletId, validUntil, msgSeqno) = (cs.loadUint(32), cs.loadUint(32), cs.loadUint(32));
  assert(!(validUntil <= blockchain.now())) throw 36;
  var ds = contract.getData().beginParse();
  var (storedSeqno, storedSubwallet, publicKey, plugins) = (ds.loadUint(32), ds.loadUint(32), ds.loadUint(256), ds.loadDict());
  ds.assertEnd();
  assert(msgSeqno == storedSeqno) throw 33;
  assert(subwalletId == storedSubwallet) throw 34;
  assert(isSignatureValid(inMsg.hash(), signature, publicKey)) throw 35;
  acceptExternalMessage();
  contract.setData(beginCell()
    .storeUint(storedSeqno + 1, 32)
    .storeUint(storedSubwallet, 32)
    .storeUint(publicKey, 256)
    .storeDict(plugins)
    .endCell());
  commitContractDataAndActions();
  cs.stackMoveToTop();
  var op: int = cs.loadUint(8);

  if (op == 0) { // simple send
    while (cs.remainingRefsCount() != 0) {
      var mode = cs.loadUint(8);
      sendRawMessage(cs.loadRef(), mode);
    }
    return; // have already saved the storage
  }

  if (op == 1) { // deploy and install plugin
    var pluginWorkchain: int = cs.loadInt(8);
    var pluginBalance: int = cs.loadCoins();
    var (stateInit: cell, body: cell) = (cs.loadRef(), cs.loadRef());
    var pluginAddress: int = stateInit.hash();
    var wcNAddress: address = beginCell().storeInt(pluginWorkchain, 8).storeUint(pluginAddress, 256).endCell().beginParse() as address;
    var msg = beginCell()
      .storeUint(0x18, 6)
      .storeUint(4, 3).storeAddress(wcNAddress)
      .storeCoins(pluginBalance)
      .storeUint(4 + 2 + 1, 1 + 4 + 4 + 64 + 32 + 1 + 1 + 1)
      .storeRef(stateInit)
      .storeRef(body);
    sendRawMessage(msg.endCell(), 3);
    var (newPlugins1: cell, isSuccess1: int) = plugins.isDictAddBuilder(8 + 256, wcNAddress as slice, beginCell());
    plugins = newPlugins1 as dict;
    assert(isSuccess1 != 0) throw 39;
  }

  if (op == 2) { // install plugin
    var wcNAddress: address = cs.loadBits(8 + 256) as address;
    var amount: int = cs.loadCoins();
    var queryId: int = cs.loadUint(64);

    var (newPlugins2: cell, isSuccess2: int) = plugins.isDictAddBuilder(8 + 256, wcNAddress as slice, beginCell());
    plugins = newPlugins2 as dict;
    assert(isSuccess2 != 0) throw 39;

    var msg: builder = beginCell()
      .storeUint(0x18, 6)
      .storeUint(4, 3).storeAddress(wcNAddress)
      .storeCoins(amount)
      .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
      .storeUint(0x6e6f7465, 32) // op
      .storeUint(queryId, 64);
    sendRawMessage(msg.endCell(), 3);
  }

  if (op == 3) { // remove plugin
    var wcNAddress: address = cs.loadBits(8 + 256) as address;
    var amount: int = cs.loadCoins();
    var queryId: int = cs.loadUint(64);

    var (newPlugins3: cell, isSuccess3: int) = plugins.isDictDelete(8 + 256, wcNAddress as slice);
    plugins = newPlugins3 as dict;
    assert(isSuccess3 != 0) throw 39;

    var msg: builder = beginCell()
      .storeUint(0x18, 6)
      .storeUint(4, 3).storeAddress(wcNAddress)
      .storeCoins(amount)
      .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
      .storeUint(0x64737472, 32) // op
      .storeUint(queryId, 64);
    sendRawMessage(msg.endCell(), 3);
  }

  contract.setData(beginCell()
    .storeUint(storedSeqno + 1, 32)
    .storeUint(storedSubwallet, 32)
    .storeUint(publicKey, 256)
    .storeDict(plugins)
    .endCell());
}

// Get methods

get fun seqno(): int {
  return contract.getData().beginParse().preloadUint(32);
}

get fun get_subwallet_id(): int {
  return contract.getData().beginParse()/* _WARNING_ method .skipBits() now mutates the object and returns self */.skipBits(32).preloadUint(32);
}

get fun get_public_key(): int {
  var cs = contract.getData().beginParse()/* _WARNING_ method .skipBits() now mutates the object and returns self */.skipBits(64);
  return cs.preloadUint(256);
}

get fun is_plugin_installed(wc: int, addrHash: int): int {
  var ds = contract.getData().beginParse()/* _WARNING_ method .skipBits() now mutates the object and returns self */.skipBits(32 + 32 + 256);
  var plugins = ds.loadDict();
  var (_, isSuccess) = plugins.isDictGet(8 + 256, beginCell().storeInt(wc, 8).storeUint(addrHash, 256).endCell().beginParse());
  return isSuccess;
}

get fun get_plugin_list(): tuple? {
  var list: tuple? = null;
  var ds = contract.getData().beginParse()/* _WARNING_ method .skipBits() now mutates the object and returns self */.skipBits(32 + 32 + 256);
  var plugins = ds.loadDict();
  var f: bool = true;
  while (f) {
    var (wcNAddress, _, found) = plugins.sDictDeleteFirstAndGet(8 + 256);
    f = found;
    if (f) {
      var wcSlice: slice = wcNAddress!;
      var (wc: int, addr: int) = (wcSlice.loadInt(8), wcSlice.loadUint(256));
      list = [[wc, addr], list] as tuple?; // manual lisp-list cons (head, tail), same shape as FunC's list_prepend
    }
  }
  return list;
}
