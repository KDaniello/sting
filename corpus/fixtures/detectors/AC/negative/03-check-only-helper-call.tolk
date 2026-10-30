fun assertSenderIsAdmin(senderAddress: address, adminAddress: address) {
    assert (senderAddress == adminAddress) throw 401;
}

fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    assertSenderIsAdmin(in.senderAddress, storage.adminAddress);
    storage.balance -= 1;
    storage.save();
}
