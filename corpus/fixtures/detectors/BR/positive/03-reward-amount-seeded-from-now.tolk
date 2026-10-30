fun computeRandomBonus(maxBonus: int): int {
    return maxBonus % blockchain.now();
}
