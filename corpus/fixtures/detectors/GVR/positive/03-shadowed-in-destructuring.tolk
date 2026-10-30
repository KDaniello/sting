contract Vesting {
    author: "sting fixture"
    storage: VestingStorage
}

struct VestingStorage {
    owner: address
    cliffTimestamp: int
}

fun readPair(): (address, int) {
    return (someAddress, someInt);
}

fun onInternalMessage(in: InMessage) {
    var (owner, extra) = readPair();
    assert (extra > 0) throw 102;
}
