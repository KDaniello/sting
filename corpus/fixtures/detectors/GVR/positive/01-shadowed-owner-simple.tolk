contract Wallet {
    author: "sting fixture"
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    balance: coins
}

fun onInternalMessage(in: InMessage) {
    var owner = in.senderAddress;
    assert (owner != null) throw 101;
}
