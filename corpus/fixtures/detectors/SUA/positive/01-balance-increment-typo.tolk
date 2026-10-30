fun creditBalance(currentBalance: int, deposit: int): int {
    var newBalance: int = currentBalance;
    newBalance =+ deposit;
    return newBalance;
}
