// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

struct MultisigState {
    ownerAddress: address
    orderSeqno: int
}

fun MultisigState.load() {
    var s = contract.getData().beginParse();
    val ownerAddress = s.loadAddress();
    val orderSeqno = s.loadUint(32);
    s.assertEnd();
    return MultisigState { ownerAddress, orderSeqno };
}

fun MultisigState.save(self) {
    contract.setData(
        beginCell().storeAddress(self.ownerAddress).storeUint(self.orderSeqno, 32).endCell()
    );
}

fun saveOrderData(orderId: int, approvalsMask: int, expiresAt: int) {
    var b = beginCell().storeUint(orderId, 64).storeUint(approvalsMask, 32).storeUint(expiresAt, 48);
    contract.setData(b.endCell());
}

fun loadOrderData() {
    var s = contract.getData().beginParse();
    val orderId = s.loadUint(64);
    val approvalsMask = s.loadUint(32);
    val expiresAt = s.loadUint(48);
    s.assertEnd();
    return (orderId, approvalsMask, expiresAt);
}

fun saveSignerData(signerIndex: int, signerAddress: address) {
    var b = beginCell().storeUint(signerIndex, 8).storeAddress(signerAddress);
    contract.setData(b.endCell());
}

fun loadSignerData() {
    var s = contract.getData().beginParse();
    val signerIndex = s.loadUint(8);
    val signerAddress = s.loadAddress();
    s.assertEnd();
    return (signerIndex, signerAddress);
}

fun parseApprovalRequest(body: slice): int {
    val orderId = body.loadUint(64);
    val signerIndex = body.loadUint(8);
    body.assertEnd();
    return orderId + signerIndex;
}

fun parseCancelRequest(body: slice): int {
    val orderId = body.loadUint(64);
    body.assertEnd();
    return orderId;
}

fun onInternalMessage(in: InMessage) {
    var storage = MultisigState.load();
    storage.orderSeqno += 1;
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
    var storage = MultisigState.load();
    storage.orderSeqno -= 1;
    storage.save();
}
