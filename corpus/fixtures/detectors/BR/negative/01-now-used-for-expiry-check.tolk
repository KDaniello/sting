fun isExpired(deadline: int): bool {
    return blockchain.now() > deadline;
}
