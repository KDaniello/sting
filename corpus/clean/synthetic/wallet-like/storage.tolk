struct WalletState {
    seqno: int
    publicKey: int
    balance: coins
}

fun WalletState.load() {
    var s = contract.getData().beginParse();
    val seqno = s.loadUint(32);
    val publicKey = s.loadUint(256);
    val balance = s.loadCoins();
    s.assertEnd();
    return WalletState { seqno, publicKey, balance };
}

fun WalletState.save(self) {
    contract.setData(
        beginCell()
            .storeUint(self.seqno, 32)
            .storeUint(self.publicKey, 256)
            .storeCoins(self.balance)
            .endCell()
    );
}
