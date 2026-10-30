contract Subscription {
    author: "sting fixture"
    storage: SubscriptionStorage
}

struct SubscriptionStorage {
    beneficiary: address
    value: int
}

fun onInternalMessage(myBalance: int, msgValue: int, msgFull: cell, msgBody: slice) {
    var storage = lazy SubscriptionStorage.load();
    var cs = msgFull.beginParse();
    val sender = cs.loadAddress();
    if (sender != storage.beneficiary) {
        return;
    }
    storage.value += 1;
    storage.save();
}
