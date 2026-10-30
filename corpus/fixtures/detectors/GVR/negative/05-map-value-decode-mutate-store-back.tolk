contract Factory {
    author: "sting fixture"
    storage: FactoryStorage
}

struct PendingDeployment {
    admin: address
    deploymentId: int32
}

struct FactoryStorage {
    owner: address
    pending: dict<int, cell>
}

fun handleDeployConfirmed(in: InMessage, key: int) {
    var storage = lazy FactoryStorage.load();
    if (!storage.pending.exists(key)) {
        return;
    }
    var pending = storage.pending.mustGet(key).load();
    pending.deploymentId += 1;
    storage.pending.set(key, pending.toCell());
    storage.save();
}
