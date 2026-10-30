contract Wallet {
    author: "sting fixture"
    storage: WalletStorage
}

struct WalletStorage {
    ownerAddress: address
    value: int
}

fun onInternalMessage(in: InMessage) {
    var storage = lazy WalletStorage.load();
    val sender = in.senderAddress;
    assert (sender == storage.ownerAddress) throw 100;
    storage.value += 1;
    storage.save();
}
