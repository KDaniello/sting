import "storage"

fun processDeposit(amount: int) {
    var storage = lazy Storage.load();
    storage.balance += amount;
    storage.save();
}
