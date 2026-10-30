import "@stdlib/gas-payments"

// Simple subscription plugin for wallet-v4
// anyone can ask to send a subscription payment

@pure
fun isSliceDataEqual(s1: slice, s2: slice): int
    asm "SDEQ";

@pure
fun `op:destruct`(): int
    asm "0x64737472 PUSHINT";
@pure
fun `op:payment_request`(): int
    asm "0x706c7567 PUSHINT";
@pure
fun `op:fallback`(): int
    asm "0x756e6b77 PUSHINT";
@pure
fun `op:subscription`(): int
    asm "0x73756273 PUSHINT";
@pure
fun maxFailedAttempts(): int
    asm "2 PUSHINT";
@pure
fun maxReservedFunds(): int
    asm "67108864 PUSHINT"; // 0.0671 TON

@inline
fun shortMsgFwdFee(workchain: int): int {
  var configIndex: int = 25 + workchain;
  var lumpPrice: int = blockchain.configParam(configIndex)!.beginParse()/* _WARNING_ method .skipBits() now mutates the object and returns self */.skipBits(8).preloadUint(64);
  return lumpPrice;
}

@inline_ref
fun gasToCoins(workchain: int, gas: int): int {
  var configIndex: int = 21 + workchain;
  var cs = blockchain.configParam(configIndex)!.beginParse();
  if (cs.preloadUint(8) == 0xd1) { // gas_flat_pfx
    cs.skipBits(8 + 64 + 64);
  }
  var tag: int = cs.loadUint(8);
  assert((tag == 0xdd) | (tag == 0xde)) throw 71; // gas_prices or gas_prices_ext
  var gasPrice: int = cs.loadUint(64);
  return (gas * gasPrice) >> 16;
}

// storage$_ wallet:MsgAddressInt
//           beneficiary:MsgAddressInt
//           amount:Grams
//           period:uint32 start_time:uint32 timeout:uint32
//           last_payment_time:uint32
//           last_request_time:uint32
//           failed_attempts:uint8
//           subscription_id:uint32 = Storage;

@inline_ref
fun loadStorage(): (address, address, int, int, int, int, int, int, int, int) {
  var ds = contract.getData().beginParse();
  return (ds.loadAddress(), ds.loadAddress(), ds.loadCoins(),
          ds.loadUint(32), ds.loadUint(32), ds.loadUint(32),
          ds.loadUint(32), ds.loadUint(32), ds.loadUint(8), ds.loadUint(32));
}

@inline_ref
fun saveStorage(wallet: address, beneficiary: address, amount: int,
                period: int, startTime: int, timeout: int, lastPaymentTime: int,
                lastRequestTime: int, failedAttempts: int, subscriptionId: int) {
  contract.setData(beginCell()
    .storeSlice(wallet as slice)
    .storeSlice(beneficiary as slice)
    .storeCoins(amount)
    .storeUint(period, 32)
    .storeUint(startTime, 32)
    .storeUint(timeout, 32)
    .storeUint(lastPaymentTime, 32)
    .storeUint(lastRequestTime, 32)
    .storeUint(failedAttempts, 8)
    .storeUint(subscriptionId, 32) // to differ subscriptions to the same beneficiary (acts as a nonce)
  .endCell());
}

@inline_ref
fun forwardFunds(beneficiary: address, isSelfDestruct: bool, op: int) {
  if (!isSelfDestruct) {
    reserveToncoinsOnBalance(maxReservedFunds(), 2); // reserve at most `max_reserved_funds` nanocoins
  }
  var [_, extraCurrencies] = contract.getOriginalBalanceWithExtraCurrencies();
  var msg = beginCell()
      .storeUint(0x10, 6) // non-bounce message
      .storeSlice(beneficiary as slice)
      .storeCoins(0)
      .storeDict(extraCurrencies.toLowLevelDict())
      .storeUint(0, 4 + 4 + 64 + 32 + 1 + 1)
      .storeUint(op, 32);
  var mode: int = 128; // carry all the remaining balance of the current smart contract
  if (isSelfDestruct) {
    mode += 32; // must be destroyed if its resulting balance is zero
  }
  sendRawMessage(msg.endCell(), mode);
}

@inline_ref
fun requestPayment(wallet: address, requestedAmount: int) {
  var (wc: int, _) = wallet.getWorkchainAndHash();
  var amount: int = gasToCoins(wc, 15000) + shortMsgFwdFee(wc);

  var msg = beginCell().storeUint(0x18, 6)
                        .storeSlice(wallet as slice)
                        .storeCoins(amount)
                        .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
                        .storeUint(`op:payment_request`(), 32) // request op
                        .storeUint(blockchain.logicalTime(), 64) // query_id
                        .storeCoins(requestedAmount)
                        .storeUint(0, 1); // empty extra
  sendRawMessage(msg.endCell(), 3);
}

fun selfDestruct(wallet: address, beneficiary: address) {
  // send event to wallet
  var (wc: int, _) = wallet.getWorkchainAndHash();
  var amount: int = gasToCoins(wc, 10000);

  var msg = beginCell().storeUint(0x10, 6) // non-bounce - we dont need answer from wallet
                        .storeSlice(wallet as slice)
                        .storeCoins(amount)
                        .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
                        .storeUint(`op:destruct`(), 32) // request op
                        .storeUint(blockchain.logicalTime(), 64); // query_id
  sendRawMessage(msg.endCell(), 3);

  // forward all the remaining funds to the beneficiary & destroy

  forwardFunds(beneficiary, true, `op:destruct`());
}

// in the future, use: fun onInternalMessage(in: InMessage) {
fun onInternalMessage(msgValue: int, inMsgCell: cell, inMsg: slice) {
  var (wallet, beneficiary, amount, period, startTime, timeout, lastPaymentTime, lastRequestTime, failedAttempts, subscriptionId) = loadStorage();
  var cs = inMsgCell.beginParse();
  var flags = cs.loadUint(4);
  var sAddr: address = cs.loadAddress();

  if (isSliceDataEqual(sAddr as slice, beneficiary as slice) != 0) {
    var op: int = inMsg.loadUint(32);
    if (op == `op:destruct`()) {
        // end subscription
        return selfDestruct(wallet, beneficiary);
    }
    return forwardFunds(beneficiary, false, `op:fallback`());
  }
  if (isSliceDataEqual(sAddr as slice, wallet as slice) == 0) {
    return forwardFunds(beneficiary, false, `op:fallback`());
  }
  if (inMsg.remainingBitsCount() < 32) {
    return forwardFunds(beneficiary, false, `op:fallback`());
  }
  var op: int = inMsg.loadUint(32);

  if (op == (`op:payment_request`() | 0x80000000)) {
    var lastTimeslot: int = (lastPaymentTime - startTime) / period;
    var curTimeslot: int = (blockchain.now() - startTime) / period;
    assert(!(lastTimeslot >= curTimeslot)) throw 49;
    var (fromWc: int, _) = sAddr.getWorkchainAndHash();

    if (msgValue >= amount - shortMsgFwdFee(fromWc) ) {
      lastPaymentTime = blockchain.now();
      failedAttempts = 0;
      forwardFunds(beneficiary, false, `op:subscription`());
    }

    return saveStorage(wallet, beneficiary, amount, period, startTime, timeout, lastPaymentTime, lastRequestTime, failedAttempts, subscriptionId);
  }
  if (op == `op:destruct`()) { // self-destruct
    // forward all the remaining funds to the beneficiary & destroy
    return forwardFunds(beneficiary, true, `op:destruct`());
  }
}

fun onExternalMessage(inMsg: slice) {
  var (wallet, beneficiary, amount, period, startTime, timeout, lastPaymentTime, lastRequestTime, failedAttempts, subscriptionId) = loadStorage();
  var lastTimeslot: int = (lastPaymentTime - startTime) / period;
  var curTimeslot: int = (blockchain.now() - startTime) / period;
  assert((curTimeslot > lastTimeslot) & (lastRequestTime + timeout < blockchain.now())) throw 30; // too early request
  acceptExternalMessage();
  if (failedAttempts >= maxFailedAttempts()) {
    selfDestruct(wallet, beneficiary);
  } else {
    requestPayment(wallet, amount);
    failedAttempts += 1;
  }
  saveStorage(wallet, beneficiary, amount, period, startTime, timeout, lastPaymentTime, blockchain.now(), failedAttempts, subscriptionId);
}

// Get methods

get fun get_subscription_data(): ([int, int], [int, int], int, int, int, int, int, int, int, int) {
  var (wallet, beneficiary, amount, period, startTime, timeout, lastPaymentTime, lastRequestTime, failedAttempts, subscriptionId) = loadStorage();
  var (walletWc: int, walletHash: int) = wallet.getWorkchainAndHash();
  var (beneficiaryWc: int, beneficiaryHash: int) = beneficiary.getWorkchainAndHash();
  return ([walletWc, walletHash], [beneficiaryWc, beneficiaryHash],
          amount, period, startTime, timeout, lastPaymentTime, lastRequestTime, failedAttempts, subscriptionId);
}
