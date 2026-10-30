fun computeUserPayout(poolAmount: int, totalShares: int, userShares: int): int {
    return poolAmount * userShares / totalShares;
}
