// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.
// Manual (non-struct) storage load/save pairs, single-function shape
// (docs/blocks/02-detectors-v1.md's ID v1 scope), consistently ordered.

fun saveMinterData(totalSupply: int, adminAddress: address) {
    var b = beginCell().storeCoins(totalSupply).storeAddress(adminAddress);
    contract.setData(b.endCell());
}

fun loadMinterData() {
    var s = contract.getData().beginParse();
    val totalSupply = s.loadCoins();
    val adminAddress = s.loadAddress();
    s.assertEnd();
    return (totalSupply, adminAddress);
}

fun saveWalletData(jettonBalance: coins, ownerAddress: address, minterAddress: address) {
    var b = beginCell()
        .storeCoins(jettonBalance)
        .storeAddress(ownerAddress)
        .storeAddress(minterAddress);
    contract.setData(b.endCell());
}

fun loadWalletData() {
    var s = contract.getData().beginParse();
    val jettonBalance = s.loadCoins();
    val ownerAddress = s.loadAddress();
    val minterAddress = s.loadAddress();
    s.assertEnd();
    return (jettonBalance, ownerAddress, minterAddress);
}

fun saveAuctionData(minBid: coins, endTime: int, bidderAddress: address) {
    var b = beginCell()
        .storeCoins(minBid)
        .storeUint(endTime, 32)
        .storeAddress(bidderAddress);
    contract.setData(b.endCell());
}

fun loadAuctionData() {
    var s = contract.getData().beginParse();
    val minBid = s.loadCoins();
    val endTime = s.loadUint(32);
    val bidderAddress = s.loadAddress();
    s.assertEnd();
    return (minBid, endTime, bidderAddress);
}
