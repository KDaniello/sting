fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    storage.balance -= 1;
    storage.save();
}
