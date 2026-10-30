import "storage"

contract Wallet {
    author: "test"
    storage: Storage
}

fun onInternalMessage(in: InMessage) {
    var balance: coins = 0;
    balance += 1;
}
