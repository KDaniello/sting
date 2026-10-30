contract Wallet {
    author: "sting fixture"
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    balance: coins
}

fun onInternalMessage(in: InMessage) {
    var storage = lazy WalletStorage.load();
    storage.balance -= 1;
    storage.save();
}
