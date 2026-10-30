fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    assert (in.senderAddress == storage.ownerAddress) throw 401;
    storage.balance -= 1;
    storage.save();
}
