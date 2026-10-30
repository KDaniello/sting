import "jetton-utils"
import "discovery-params"
import "op-codes"
import "params"

// Jettons minter smart contract

// storage scheme
// storage#_ total_supply:Coins admin_address:MsgAddress content:^Cell jetton_wallet_code:^Cell = Storage;

@inline
fun loadData(): (int, address, cell, cell) {
  var ds: slice = contract.getData().beginParse();
  return (
      ds.loadCoins(), // total_supply
      ds.loadAddress(), // admin_address
      ds.loadRef(), // content
      ds.loadRef()  // jetton_wallet_code
  );
}

@inline
fun saveData(totalSupply: int, adminAddress: address, content: cell, jettonWalletCode: cell) {
  contract.setData(beginCell()
            .storeCoins(totalSupply)
            .storeAddress(adminAddress)
            .storeRef(content)
            .storeRef(jettonWalletCode)
           .endCell()
          );
}

fun mintTokens(toAddress: address, jettonWalletCode: cell, amount: int, masterMsg: cell) {
  var stateInit: cell = calculateJettonWalletStateInit(toAddress, contract.getAddress(), jettonWalletCode);
  var toWalletAddress: address = calculateJettonWalletAddress(stateInit);
  var msg = beginCell()
    .storeUint(0x18, 6)
    .storeAddress(toWalletAddress)
    .storeCoins(amount)
    .storeUint(4 + 2 + 1, 1 + 4 + 4 + 64 + 32 + 1 + 1 + 1)
    .storeRef(stateInit)
    .storeRef(masterMsg);
  sendRawMessage(msg.endCell(), 1); // pay transfer fees separately, revert on errors
}

// in the future, use: fun onInternalMessage(in: InMessage) {
fun onInternalMessage(msgValue: int, inMsgFull: cell, inMsgBody: slice) {
    var cs: slice = inMsgFull.beginParse();
    var flags: int = cs.loadUint(4);

    if ((flags & 1) != 0) { // ignore all bounced messages
        return;
    }
    var senderAddress: address = cs.loadAddress();

    var (totalSupply: int, adminAddress: address, content: cell, jettonWalletCode: cell) = loadData();

    if (inMsgBody.isEmpty()) { // buy jettons for Toncoin

      var amount: int = 10000000; // for mint message
      var buyAmount: int = msgValue - amount;
      assert(buyAmount > 0) throw 76;

      var jettonAmount: int = buyAmount; // rate 1 jetton = 1 toncoin; multiply to price here

      var masterMsg = beginCell()
            .storeUint(OP_INTERNAL_TRANSFER(), 32)
            .storeUint(0, 64) // quert_id
            .storeCoins(jettonAmount)
            .storeAddress(contract.getAddress()) // from_address
            .storeAddress(senderAddress) // response_address
            .storeCoins(0) // no forward_amount
            .storeUint(0, 1) // forward_payload in this slice, not separate cell
            .endCell();

      mintTokens(senderAddress, jettonWalletCode, amount, masterMsg);
      saveData(totalSupply + jettonAmount, adminAddress, content, jettonWalletCode);
      return;
    }

    var op: int = inMsgBody.loadUint(32);
    var queryId: int = inMsgBody.loadUint(64);

    if (op == OP_MINT()) {
        assert(senderAddress.bitsEqual(adminAddress)) throw 73;
        var toAddress: address = inMsgBody.loadAddress();
        var amount: int = inMsgBody.loadCoins();
        var masterMsg: cell = inMsgBody.loadRef();
        var masterMsgCs: slice = masterMsg.beginParse();
        masterMsgCs.skipBits(32 + 64); // op + query_id
        var jettonAmount: int = masterMsgCs.loadCoins();
        mintTokens(toAddress, jettonWalletCode, amount, masterMsg);
        saveData(totalSupply + jettonAmount, adminAddress, content, jettonWalletCode);
        return;
    }

    if (op == OP_BURN_NOTIFICATION()) {
        var jettonAmount: int = inMsgBody.loadCoins();
        var fromAddress: address = inMsgBody.loadAddress();
        assert(calculateUserJettonWalletAddress(fromAddress, contract.getAddress(), jettonWalletCode).bitsEqual(senderAddress)) throw 74;
        saveData(totalSupply - jettonAmount, adminAddress, content, jettonWalletCode);
        var responseAddress: address = inMsgBody.loadAddress();
        if ((responseAddress as slice).preloadUint(2) != 0) {
          var msg = beginCell()
            .storeUint(0x10, 6) // nobounce - int_msg_info$0 ihr_disabled:Bool bounce:Bool bounced:Bool src:MsgAddress -> 011000
            .storeAddress(responseAddress)
            .storeCoins(0)
            .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
            .storeUint(OP_EXCESSES(), 32)
            .storeUint(queryId, 64);
          sendRawMessage(msg.endCell(), 2 + 64);
        }
        return;
    }

    throw 0xffff;
}

get fun get_jetton_data(): (int, int, slice, cell, cell) {
    var (totalSupply: int, adminAddress: address, content: cell, jettonWalletCode: cell) = loadData();
    return (totalSupply, -1, adminAddress as slice, content, jettonWalletCode);
}

get fun get_wallet_address(ownerAddress: address): address {
    var (totalSupply: int, adminAddress: address, content: cell, jettonWalletCode: cell) = loadData();
    return calculateUserJettonWalletAddress(ownerAddress, contract.getAddress(), jettonWalletCode);
}
