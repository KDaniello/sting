fun onInternalMessage(in: InMessage) {
    val recipient: slice = in.body.loadAddress();
    val out = createMessage({
        bounce: BounceMode.Only256BitsOfBody,
        dest: recipient,
        value: 0,
        body: y,
    });
    out.send(64);
}
