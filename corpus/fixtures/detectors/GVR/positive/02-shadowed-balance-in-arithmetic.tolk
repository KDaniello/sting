contract Jetton {
    author: "sting fixture"
    storage: JettonStorage
}

struct JettonStorage {
    balance: coins
    adminAddress: address
}

fun applyDiscount(rawAmount: int) {
    var balance = rawAmount - rawAmount / 10;
    someExternalCall(balance);
}
