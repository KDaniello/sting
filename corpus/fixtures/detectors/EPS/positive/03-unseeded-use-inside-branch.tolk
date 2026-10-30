fun claimRandomReward(isEligible: bool): int {
    if (isEligible) {
        return random.uint256();
    }
    return 0;
}
