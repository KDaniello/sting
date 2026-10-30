// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct HighloadState {
    lastCleanTime: int
    publicKey: int
}

fun HighloadState.load() {
    var s = contract.getData().beginParse();
    val lastCleanTime = s.loadUint(32);
    val publicKey = s.loadUint(256);
    s.assertEnd();
    return HighloadState { lastCleanTime, publicKey };
}

fun HighloadState.save(self) {
    contract.setData(
        beginCell().storeUint(self.lastCleanTime, 32).storeUint(self.publicKey, 256).endCell()
    );
}

fun onInternalMessage(in: InMessage) {
    var storage = HighloadState.load();
    storage.lastCleanTime += 1;
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
    var storage = HighloadState.load();
    storage.lastCleanTime -= 1;
    storage.save();
}
