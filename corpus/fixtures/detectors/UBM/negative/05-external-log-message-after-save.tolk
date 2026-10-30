fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    storage.balance -= 1;
    storage.save();

    val event = createExternalLogMessage({
        dest: ExtOutLogBucket { topic: 0x00de9052 },
        body: DepositEvent { queryId: 1 }
    });
    event.send(SEND_MODE_REGULAR);
}
