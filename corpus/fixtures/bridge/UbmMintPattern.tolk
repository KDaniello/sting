// Hand-authored, not vendored — see corpus/fixtures/bridge/SOURCE.md for why.
// The same UBM pattern as corpus/fixtures/detectors/UBM/positive/03-real-jetton-mint-pattern.tolk
// (itself a direct isolated copy of the real ton-blockchain/tolk-bench
// JettonMinter.tolk mint chunk, docs/blocks/02-detectors-v1.md), with one
// deliberate difference: dispatch is by opcode only, not gated behind
// `in.senderAddress == storage.adminAddress`. Confirmed by direct
// experimentation during Block 07's implementation: `INMSG_SRC` (and
// `INMSG_BOUNCED`) do not populate correctly through
// `TvmEmulator::send_internal_message`'s reduced execution context (no real
// transaction/collator context, just a direct method-call emulation) — so a
// sender-gated finding is not dynamically reachable through that API at
// all, independent of how carefully the caller's message is constructed.
// See docs/blocks/07-static-dynamic-bridge.md's Результаты for the full
// finding. This fixture isolates the *storage-commits-before-a-bounceable-send*
// half of the UBM pattern, which the same investigation confirmed IS
// reliably reachable and verifiable this way.

const OP_MINT = 0x4d494e54; // "MINT"

struct (0x00000001) Ping {
    queryId: uint64
}

struct Storage {
    totalSupply: coins
}

fun Storage.load() {
    var s = contract.getData().beginParse();
    val totalSupply = s.loadCoins();
    s.assertEnd();
    return Storage { totalSupply };
}

fun Storage.save(self) {
    contract.setData(beginCell().storeCoins(self.totalSupply).endCell());
}

fun onInternalMessage(in: InMessage) {
    val op = in.body.loadUint(32);
    if (op == OP_MINT) {
        val mintAmount = in.body.loadCoins();

        var storage = Storage.load();
        storage.totalSupply += mintAmount;
        storage.save();

        // BUG (UBM class, docs/blocks/02-detectors-v1.md): storage is
        // committed above, then a bounceable message is sent below, but
        // this file defines no `onBouncedMessage` handler at all — if the
        // send bounces, the totalSupply increase is never reverted.
        val deployMsg = createMessage({
            bounce: BounceMode.Only256BitsOfBody,
            dest: contract.getAddress(),
            value: 1,
            body: Ping { queryId: 0 },
        });
        deployMsg.send(SEND_MODE_PAY_FEES_SEPARATELY);
    }
}

get fun getTotalSupply(): int {
    return Storage.load().totalSupply;
}
