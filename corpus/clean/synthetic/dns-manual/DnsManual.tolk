// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct DomainState {
    ownerAddress: address
    expiresAt: int
}

fun DomainState.load() {
    var s = contract.getData().beginParse();
    val ownerAddress = s.loadAddress();
    val expiresAt = s.loadUint(32);
    s.assertEnd();
    return DomainState { ownerAddress, expiresAt };
}

fun DomainState.save(self) {
    contract.setData(
        beginCell().storeAddress(self.ownerAddress).storeUint(self.expiresAt, 32).endCell()
    );
}

fun saveAuctionData(minBid: coins, endTime: int, lastBidderAddress: address) {
    var b = beginCell()
        .storeCoins(minBid)
        .storeUint(endTime, 32)
        .storeAddress(lastBidderAddress);
    contract.setData(b.endCell());
}

fun loadAuctionData() {
    var s = contract.getData().beginParse();
    val minBid = s.loadCoins();
    val endTime = s.loadUint(32);
    val lastBidderAddress = s.loadAddress();
    s.assertEnd();
    return (minBid, endTime, lastBidderAddress);
}

fun saveResolverData(recordCount: int, updatedAt: int) {
    var b = beginCell().storeUint(recordCount, 16).storeUint(updatedAt, 32);
    contract.setData(b.endCell());
}

fun loadResolverData() {
    var s = contract.getData().beginParse();
    val recordCount = s.loadUint(16);
    val updatedAt = s.loadUint(32);
    s.assertEnd();
    return (recordCount, updatedAt);
}

fun saveNameRecordData(recordType: int, ttl: int) {
    var b = beginCell().storeUint(recordType, 8).storeUint(ttl, 32);
    contract.setData(b.endCell());
}

fun loadNameRecordData() {
    var s = contract.getData().beginParse();
    val recordType = s.loadUint(8);
    val ttl = s.loadUint(32);
    s.assertEnd();
    return (recordType, ttl);
}

fun parseBidRequest(body: slice): coins {
    val bidAmount = body.loadCoins();
    body.assertEnd();
    return bidAmount;
}

fun parseRenewalRequest(body: slice): int {
    val extraDays = body.loadUint(16);
    body.assertEnd();
    return extraDays;
}

fun onInternalMessage(in: InMessage) {
    var storage = DomainState.load();
    storage.expiresAt += 1;
    storage.save();

    val out = createMessage({
        bounce: BounceMode.Only256BitsOfBody,
        dest: in.senderAddress,
        value: 0,
        body: ReturnExcessesBack { queryId: 0 },
    });
    out.send(SEND_MODE_PAY_FEES_SEPARATELY);
}

fun onBouncedMessage(in: InMessageBounced) {
    var storage = DomainState.load();
    storage.expiresAt -= 1;
    storage.save();
}
