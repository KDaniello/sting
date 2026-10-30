//! The decoder against a captured mainnet message, checked by somebody else's decode.
//!
//! The body below is copied verbatim out of `corpus/sybil/fixtures/mc_block_78805592.json`, and the
//! values it is checked against are that same response's own decoding of it, produced by an
//! endpoint with nothing in common with this code. Two independent decoders agreeing on a real
//! message is evidence; a decoder agreeing with itself is not.
//!
//! What is being checked here is not the cell library — that is somebody else's tested code — but
//! the join between a learned dataset and a wire: that the opcode this body opens with is looked up
//! as an operation, and that the fields that operation was declared to carry line up, in order,
//! with the bits actually on the wire.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use base64::Engine as _;
use sting_abi_decode::{DecodeError, Value, decode};
use sting_abi_dictionary::OperationDictionary;
use tonlib_core::cell::{BagOfCells, CellBuilder};

/// The body of one outgoing message of a real transaction, at line 635 of the capture.
const REAL_JETTON_NOTIFY_BODY_B64: &str = "te6cckEBAgEAPgABZHNi0Jzv5G1UHv2tpUEeGjAIAAxyuxvBmtyIDdYsj3iafEOsMHxyCuREQYSBefhi2qTlAQAOAAAAADMwMD/iE3A=";

/// What the endpoint decoded that same message to, at lines 638 to 650 of the capture.
const GROUND_TRUTH_QUERY_ID: &str = "17286061477819428261";
const GROUND_TRUTH_AMOUNT: &str = "300000000";
const GROUND_TRUTH_SENDER: &str =
    "0:06395d8de0cd6e4406eb1647bc4d3e21d6183e3905722220c240bcfc316d5272";

/// The committed dataset, not a fixture built for this test.
fn committed_dictionary() -> OperationDictionary {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus-abi/operation-dictionary.json");
    OperationDictionary::load(&path).expect("the committed operation dictionary")
}

#[test]
fn a_real_mainnet_body_reads_as_the_operation_the_dictionary_knows() {
    let body = base64::engine::general_purpose::STANDARD
        .decode(REAL_JETTON_NOTIFY_BODY_B64)
        .unwrap();

    let decoded = decode(&body, &committed_dictionary()).expect("a real mainnet body must read");

    assert_eq!(decoded.opcode, 0x7362_d09c);
    assert!(
        !decoded.is_unknown(),
        "the dataset knows TEP-74's transfer notification"
    );

    // Every candidate the dataset holds for this opcode reads the same body, and each has to agree
    // with the endpoint on the three fields it names — the names differ between contracts, the
    // values on the wire do not.
    for operation in &decoded.operations {
        let values: Vec<&Value> = operation.fields.iter().map(|field| &field.value).collect();
        assert_eq!(
            values[0],
            &Value::Uint {
                value: GROUND_TRUTH_QUERY_ID.to_string()
            },
            "{} read a different query id",
            operation.name
        );
        assert_eq!(
            values[1],
            &Value::Coins {
                value: GROUND_TRUTH_AMOUNT.to_string()
            },
            "{} read a different amount",
            operation.name
        );
        assert_eq!(
            values[2],
            &Value::Address {
                value: GROUND_TRUTH_SENDER.to_string()
            },
            "{} read a different sender",
            operation.name
        );
    }
}

#[test]
fn an_opcode_no_contract_declares_is_answered_as_unknown_rather_than_guessed() {
    // Built rather than captured: what is being checked is the answer to an opcode nothing in the
    // corpus declares, and a real body would only be one such opcode until somebody deployed it.
    let mut builder = CellBuilder::new();
    builder.store_u32(32, 0xdead_beef).unwrap();
    let body = BagOfCells::from_root(builder.build().unwrap())
        .serialize(false)
        .unwrap();

    let decoded = decode(&body, &committed_dictionary()).expect("a well-formed body must read");

    assert_eq!(decoded.opcode, 0xdead_beef);
    assert!(decoded.is_unknown());
}

#[test]
fn a_body_too_short_to_carry_an_opcode_is_said_to_be_that_rather_than_decoded() {
    let body = BagOfCells::from_root(CellBuilder::new().build().unwrap())
        .serialize(false)
        .unwrap();

    let error = decode(&body, &committed_dictionary()).expect_err("no opcode to read");

    assert!(matches!(error, DecodeError::NoOpcode(0)), "{error}");
}

#[test]
fn a_bounce_says_what_bounced_rather_than_reading_a_body_that_was_cut() {
    // TON's own layout: the marker, then the first 256 bits of the body that came back — here the
    // opening of a jetton transfer.
    let mut builder = CellBuilder::new();
    builder.store_u32(32, 0xffff_ffff).unwrap();
    builder.store_u32(32, 0x0f8a_7ea5).unwrap();
    let body = BagOfCells::from_root(builder.build().unwrap())
        .serialize(false)
        .unwrap();

    let decoded = decode(&body, &committed_dictionary()).expect("a bounce must read");

    assert!(
        !decoded.is_unknown(),
        "the chain's own layout names a bounce"
    );
    assert_eq!(decoded.standard.as_deref(), Some("a bounced message"));
    let bounced = decoded.bounced.expect("a bounce says what bounced");
    assert_eq!(bounced.original_opcode, 0x0f8a_7ea5);
    assert!(
        bounced
            .original_names
            .iter()
            .any(|name| name == "AskToTransfer"),
        "{:?}",
        bounced.original_names
    );
}
