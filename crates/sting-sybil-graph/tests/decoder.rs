//! The decoder against a captured mainnet message, checked by somebody else's decode.
//!
//! The body and the opcode below are copied verbatim out of
//! `corpus/sybil/fixtures/mc_block_78805592.json`. What makes this more than a round trip is
//! where the expected values come from: the same response carries the endpoint's own decoding of
//! that message, produced by code with nothing in common with this one. Two independent decoders
//! agreeing on a real message is evidence; a decoder agreeing with itself is not.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use base64::Engine;
use sting_sybil_graph::{JettonOp, OP_TRANSFER_NOTIFICATION, decode_jetton_body};
use sting_sybil_types::TonAddress;

/// The body of one outgoing message of a real transaction, at line 635 of the capture.
const REAL_JETTON_NOTIFY_BODY_B64: &str = "te6cckEBAgEAPgABZHNi0Jzv5G1UHv2tpUEeGjAIAAxyuxvBmtyIDdYsj3iafEOsMHxyCuREQYSBefhi2qTlAQAOAAAAADMwMD/iE3A=";

/// What the endpoint decoded that same message to, at lines 638 to 650 of the capture.
const GROUND_TRUTH_QUERY_ID: u64 = 17_286_061_477_819_428_261;
const GROUND_TRUTH_AMOUNT: u128 = 300_000_000;
const GROUND_TRUTH_SENDER_HEX: &str =
    "06395D8DE0CD6E4406EB1647BC4D3E21D6183E3905722220C240BCFC316D5272";

#[test]
fn the_decoder_agrees_with_an_independent_one_on_a_real_message() {
    let body = base64::engine::general_purpose::STANDARD
        .decode(REAL_JETTON_NOTIFY_BODY_B64)
        .unwrap();

    let op =
        decode_jetton_body(OP_TRANSFER_NOTIFICATION, &body).expect("a real mainnet body must read");

    let JettonOp::TransferNotification {
        query_id,
        amount,
        sender,
    } = op
    else {
        panic!("expected a transfer notification, got {op:?}")
    };

    assert_eq!(query_id, GROUND_TRUTH_QUERY_ID);
    assert_eq!(amount, GROUND_TRUTH_AMOUNT, "the amount is read as coins");

    let expected_sender =
        TonAddress::from_raw_string(&format!("0:{}", GROUND_TRUTH_SENDER_HEX.to_lowercase()))
            .unwrap();
    assert_eq!(sender, expected_sender);
}
