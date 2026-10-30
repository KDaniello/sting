// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct VestingState {
    ownerAddress: address
    lockedAmount: coins
}

fun VestingState.load() {
    var s = contract.getData().beginParse();
    val ownerAddress = s.loadAddress();
    val lockedAmount = s.loadCoins();
    s.assertEnd();
    return VestingState { ownerAddress, lockedAmount };
}

fun VestingState.save(self) {
    contract.setData(
        beginCell().storeAddress(self.ownerAddress).storeCoins(self.lockedAmount).endCell()
    );
}

fun saveScheduleData(startTime: int, endTime: int, totalAmount: coins) {
    var b = beginCell().storeUint(startTime, 32).storeUint(endTime, 32).storeCoins(totalAmount);
    contract.setData(b.endCell());
}

fun loadScheduleData() {
    var s = contract.getData().beginParse();
    val startTime = s.loadUint(32);
    val endTime = s.loadUint(32);
    val totalAmount = s.loadCoins();
    s.assertEnd();
    return (startTime, endTime, totalAmount);
}

fun saveClaimData(claimedAmount: coins, lastClaimTime: int) {
    var b = beginCell().storeCoins(claimedAmount).storeUint(lastClaimTime, 32);
    contract.setData(b.endCell());
}

fun loadClaimData() {
    var s = contract.getData().beginParse();
    val claimedAmount = s.loadCoins();
    val lastClaimTime = s.loadUint(32);
    s.assertEnd();
    return (claimedAmount, lastClaimTime);
}

fun parseUnlockRequest(body: slice): int {
    val requestedAmount = body.loadCoins();
    val nonce = body.loadUint(32);
    body.assertEnd();
    return nonce;
}

fun parseBeneficiaryChangeRequest(body: slice): address {
    val newBeneficiary = body.loadAddress();
    body.assertEnd();
    return newBeneficiary;
}

fun onInternalMessage(in: InMessage) {
    var storage = VestingState.load();
    storage.lockedAmount -= 1;
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
    var storage = VestingState.load();
    storage.lockedAmount += 1;
    storage.save();
}
