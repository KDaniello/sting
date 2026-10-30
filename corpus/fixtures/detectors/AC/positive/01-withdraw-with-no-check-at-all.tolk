fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    val withdrawMsg = createMessage({
        bounce: BounceMode.NoBounce,
        dest: storage.ownerAddress,
        value: storage.balance,
        body: WithdrawConfirmation { queryId: 0 },
    });
    withdrawMsg.send(SEND_MODE_CARRY_ALL_REMAINING_MESSAGE_VALUE);
    storage.balance = 0;
    storage.save();
}
