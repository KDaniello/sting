fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    if (in.senderAddress != storage.ownerAddress) {
        return;
    }
    storage.balance -= 1;
    storage.save();
}
