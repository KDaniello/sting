fun pickEntry(bound: int): int {
    random.initialize();
    return random.uint256() % bound;
}
