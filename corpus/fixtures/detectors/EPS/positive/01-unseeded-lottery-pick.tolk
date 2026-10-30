fun pickLotteryWinner(participantCount: int): int {
    val winnerIndex: int = random.range(participantCount);
    return winnerIndex;
}
