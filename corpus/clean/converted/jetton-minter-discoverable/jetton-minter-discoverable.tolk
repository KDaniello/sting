import "jetton-utils"
import "discovery-params"
import "op-codes"
import "params"

// Jettons discoverable smart contract

// 6905(computational_gas_price) * 1000(cur_gas_price) = 6905000
// ceil(6905000) = 10000000 ~= 0.01 TON
@pure
fun provideAddressGasConsumption(): int
    asm "10000000 PUSHINT";

// storage scheme
// storage#_ total_supply:Coins admin_address:MsgAddress content:^Cell jetton_wallet_code:^Cell = Storage;

@inline
fun loadData(): (int, address, cell, cell) {
    var ds: slice = contract.getData().beginParse();
    return (
            ds.loadCoins(), // total_supply
            ds.loadAddress(), // admin_address
            ds.loadRef(), // content
            ds.loadRef() // jetton_wallet_code
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
    if (inMsgBody.isEmpty()) { // ignore empty messages
        return;
    }
    var cs: slice = inMsgFull.beginParse();
    var flags: int = cs.loadUint(4);

    if ((flags & 1) != 0) { // ignore all bounced messages
        return;
    }
    var senderAddress: address = cs.loadAddress();
    cs.loadAddress(); // skip dst
    cs.loadCoins(); // skip value
    cs.skipBits(1); // skip extracurrency collection
    cs.loadCoins(); // skip ihr_fee
    var fwdFee: int = mulDivFloor(cs.loadCoins(), 3, 2); // we use message fwd_fee for estimation of forward_payload costs

    var op: int = inMsgBody.loadUint(32);
    var queryId: int = inMsgBody.loadUint(64);

    var (totalSupply: int, adminAddress: address, content: cell, jettonWalletCode: cell) = loadData();

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

    if (op == OP_PROVIDE_WALLET_ADDRESS()) {
        assert(msgValue > fwdFee + provideAddressGasConsumption()) throw 75;

        var ownerAddress: address = inMsgBody.loadAddress();
        var isIncludeAddress: int = inMsgBody.loadUint(1);

        var includedAddress: cell? = isIncludeAddress != 0
                ? beginCell().storeAddress(ownerAddress).endCell()
                : null;

        var msg = beginCell()
                .storeUint(0x18, 6)
                .storeAddress(senderAddress)
                .storeCoins(0)
                .storeUint(0, 1 + 4 + 4 + 64 + 32 + 1 + 1)
                .storeUint(OP_TAKE_WALLET_ADDRESS(), 32)
                .storeUint(queryId, 64);

        if (isResolvable(ownerAddress) != 0) {
            msg = msg.storeAddress(calculateUserJettonWalletAddress(ownerAddress, contract.getAddress(), jettonWalletCode));
        } else {
            msg = msg.storeUint(0, 2); // addr_none
        }
        sendRawMessage(msg.storeMaybeRef(includedAddress).endCell(), 64);
        return;
    }

    if (op == 3) { // change admin
        assert(senderAddress.bitsEqual(adminAddress)) throw 73;
        var newAdminAddress: address = inMsgBody.loadAddress();
        saveData(totalSupply, newAdminAddress, content, jettonWalletCode);
        return;
    }

    if (op == 4) { // change content, delete this for immutable tokens
        assert(senderAddress.bitsEqual(adminAddress)) throw 73;
        saveData(totalSupply, adminAddress, inMsgBody.loadRef(), jettonWalletCode);
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
