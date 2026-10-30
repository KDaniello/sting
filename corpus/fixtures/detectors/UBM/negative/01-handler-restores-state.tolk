fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    storage.balance -= 1;
    storage.save();

    val out = createMessage({
        bounce: BounceMode.Only256BitsOfBody,
        dest: someAddress,
        value: 0,
        body: SomeMessage { queryId: 1 }
    });
    out.send(SEND_MODE_PAY_FEES_SEPARATELY);
}

fun onBouncedMessage(in: InMessageBounced) {
    var storage = lazy Storage.load();
    storage.balance += 1;
    storage.save();
}
