fun computeCrossExchangeRate(baseAmount: int, baseReserve: int, quoteAmount: int, quoteReserve: int): int {
    return (baseAmount / baseReserve) * (quoteAmount / quoteReserve);
}
