fun pickLotteryWinner(participantCount: int): int {
    random.initialize();
    val winnerIndex: int = random.range(participantCount);
    return winnerIndex;
}
