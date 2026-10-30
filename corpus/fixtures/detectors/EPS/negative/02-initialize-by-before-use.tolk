fun generateLootBoxSeed(mixIn: int): int {
    random.initializeBy(mixIn);
    return random.uint256();
}
