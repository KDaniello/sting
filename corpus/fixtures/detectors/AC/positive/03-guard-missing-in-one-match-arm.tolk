fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    val msg = lazy AllowedMessage.fromSlice(in.body);
    match (msg) {
        SetOwnerRequest => {
            assert (in.senderAddress == storage.ownerAddress) throw 401;
            storage.ownerAddress = msg.newOwnerAddress;
            storage.save();
        }
        SetMetadataRequest => {
            storage.metadata = msg.newMetadata;
            storage.save();
        }
        else => {
            return;
        }
    }
}
