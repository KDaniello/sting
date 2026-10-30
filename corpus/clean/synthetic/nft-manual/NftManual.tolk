// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct ItemState {
    ownerAddress: address
    itemIndex: int
}

fun ItemState.load() {
    var s = contract.getData().beginParse();
    val ownerAddress = s.loadAddress();
    val itemIndex = s.loadUint(32);
    s.assertEnd();
    return ItemState { ownerAddress, itemIndex };
}

fun ItemState.save(self) {
    contract.setData(
        beginCell()
            .storeAddress(self.ownerAddress)
            .storeUint(self.itemIndex, 32)
            .endCell()
    );
}

fun parseTransferRequest(body: slice): address {
    val newOwner = body.loadAddress();
    val forwardAmount = body.loadCoins();
    body.assertEnd();
    return newOwner;
}

fun parseEditRequest(body: slice): int {
    val fieldId = body.loadUint(8);
    val newValue = body.loadUint(64);
    body.assertEnd();
    return fieldId + newValue;
}

fun onInternalMessage(in: InMessage) {
    var storage = ItemState.load();
    storage.itemIndex += 1;
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
    var storage = ItemState.load();
    storage.itemIndex -= 1;
    storage.save();
}
