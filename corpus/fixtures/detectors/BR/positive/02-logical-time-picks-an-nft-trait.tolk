fun pickRandomTrait(traitCount: int): int {
    return blockchain.logicalTime() % traitCount;
}
