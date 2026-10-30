// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct NotcoinState {
    jettonBalance: coins
    ownerAddress: address
}

fun NotcoinState.load() {
    var s = contract.getData().beginParse();
    val jettonBalance = s.loadCoins();
    val ownerAddress = s.loadAddress();
    s.assertEnd();
    return NotcoinState { jettonBalance, ownerAddress };
}

fun NotcoinState.save(self) {
    contract.setData(
        beginCell().storeCoins(self.jettonBalance).storeAddress(self.ownerAddress).endCell()
    );
}

fun onInternalMessage(in: InMessage) {
    var storage = NotcoinState.load();
    storage.jettonBalance -= 1;
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
    var storage = NotcoinState.load();
    storage.jettonBalance += 1;
    storage.save();
}
