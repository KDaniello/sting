fun advanceSeqno(currentSeqno: int, step: int): int {
    var nextSeqno: int = currentSeqno;
    nextSeqno =+ step;
    return nextSeqno;
}
