fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    assert (in.senderAddress == storage.ownerAddress) throw 401;
    val newCode: cell = in.body.loadRef();
    contract.setCodePostponed(newCode);
}
