fun isSameAddress(sender: address, owner: address): bool {
    return sender.bitsEqual(owner);
}
