fun pickLotteryWinner(participantCount: int): int {
    return blockchain.now() % participantCount;
}
