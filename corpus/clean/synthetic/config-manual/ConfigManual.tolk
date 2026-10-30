// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct ConfigState {
    voteCount: int
    quorumReached: int
}

fun ConfigState.load() {
    var s = contract.getData().beginParse();
    val voteCount = s.loadUint(32);
    val quorumReached = s.loadUint(1);
    s.assertEnd();
    return ConfigState { voteCount, quorumReached };
}

fun ConfigState.save(self) {
    contract.setData(
        beginCell().storeUint(self.voteCount, 32).storeUint(self.quorumReached, 1).endCell()
    );
}

fun onInternalMessage(in: InMessage) {
    var storage = ConfigState.load();
    storage.voteCount += 1;
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
    var storage = ConfigState.load();
    storage.voteCount -= 1;
    storage.save();
}
