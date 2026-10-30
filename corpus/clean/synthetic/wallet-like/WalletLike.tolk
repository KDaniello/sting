// Hand-authored, not vendored — see corpus/clean/synthetic/SOURCE.md for why.

import "storage"

struct (0xd53276db) ReturnExcessesBack {
    queryId: uint64
}

fun onExternalMessage(inMsg: slice) {
    var signature = inMsg.loadBits(512);
    val seqno = inMsg.loadUint(32);
    assert (isSignatureValid(inMsg.hash(), signature, seqno)) throw 35;
    inMsg.assertEnd();
}

fun parseSubscriptionRequest(body: slice): int {
    val planId = body.loadUint(16);
    val durationDays = body.loadUint(16);
    body.assertEnd();
    return planId + durationDays;
}

fun parseRecoveryRequest(payload: slice): int {
    val recoveryCode = payload.loadUint(64);
    payload.assertEnd();
    return recoveryCode;
}

fun onInternalMessage(in: InMessage) {
    var storage = WalletState.load();
    storage.balance -= 1;
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
    var storage = WalletState.load();
    storage.balance += 1;
    storage.save();
}
