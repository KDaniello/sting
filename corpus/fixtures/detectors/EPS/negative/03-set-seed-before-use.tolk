fun generateWithExplicitSeed(seed: int): int {
    random.setSeed(seed);
    return random.uint256();
}
