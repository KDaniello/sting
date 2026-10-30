contract Jetton {
    author: "sting fixture"
    storage: JettonStorage
}

struct JettonStorage {
    balance: coins
    adminAddress: address
}

fun countMessages(in: InMessage) {
    var totalCount = 0;
    totalCount += 1;
}
