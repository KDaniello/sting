fun computeWeightedReward(userWeight: int, totalReward: int, totalWeight: int): int {
    return userWeight * (totalReward / totalWeight);
}
