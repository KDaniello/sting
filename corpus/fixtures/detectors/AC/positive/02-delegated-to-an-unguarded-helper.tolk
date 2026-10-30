fun setFeePercent(newFeePercent: int) {
    var storage = lazy Storage.load();
    storage.feePercent = newFeePercent;
    storage.save();
}

fun onInternalMessage(in: InMessage) {
    val msg = lazy AllowedMessage.fromSlice(in.body);
    match (msg) {
        SetFeePercentRequest => {
            setFeePercent(msg.newFeePercent);
        }
        else => {
            return;
        }
    }
}
