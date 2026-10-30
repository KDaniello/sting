contract Market {
    author: "sting fixture"
    storage: MarketStorage
}

struct MarketState {
    accumulatedFees: coins
}

struct MarketStorage {
    owner: address
    state: cell
}

fun onInternalMessage(in: InMessage) {
    var storage = lazy MarketStorage.load();
    assert (in.senderAddress == storage.owner) throw 401;
    var state = storage.state.load();
    state.accumulatedFees = 0 as coins;
    storage.state = state.toCell();
    storage.save();
}
