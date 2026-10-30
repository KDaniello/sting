fun sendNonBounceable(dest: address, value: coins, body: cell) {
    val out = createMessage({
        bounce: BounceMode.NoBounce,
        dest: dest,
        value: value,
        body: body,
    });
    out.send(SEND_MODE_REGULAR);
}
