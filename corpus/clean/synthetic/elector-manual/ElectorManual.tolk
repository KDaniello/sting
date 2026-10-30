// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct ElectorState {
    totalStake: coins
    electionId: int
}

fun ElectorState.load() {
    var s = contract.getData().beginParse();
    val totalStake = s.loadCoins();
    val electionId = s.loadUint(32);
    s.assertEnd();
    return ElectorState { totalStake, electionId };
}

fun ElectorState.save(self) {
    contract.setData(
        beginCell().storeCoins(self.totalStake).storeUint(self.electionId, 32).endCell()
    );
}

fun onInternalMessage(in: InMessage) {
    var storage = ElectorState.load();
    storage.totalStake -= 1;
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
    var storage = ElectorState.load();
    storage.totalStake += 1;
    storage.save();
}
