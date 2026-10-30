//! The two risk-signal capabilities, run against real contracts.
//!
//! Both are marked live in `docs/CAPABILITIES.md`, and both promise something narrower than a
//! verdict: every applicable signal with its own answer, and silence where a signal does not apply.
//! What is asserted here is that shape rather than a particular risk score — a score is a function
//! of a contract that changes, and pinning one would be pinning today's chain.
//!
//! Ignored by default and run by `make test-live`. They do not skip when the network is absent:
//! they are run deliberately, and a quiet pass would be worse than no test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::process::Command;

/// Tether's jetton master on this chain: the largest one there is, and the one the corpus already
/// carries a captured wallet of.
const LIVE_JETTON: &str = "EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs";

/// A real gift collection the registry knows by address, in the raw form the registry states it in.
const LIVE_COLLECTION: &str = "0:46FA0E9A864014196A5E7D66F1F83FFDB10F2859BBF2EA9BAEABBF14D9CE0D50";

#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn a_jetton_answers_with_signals_rather_than_a_verdict() {
    let output = Command::new(env!("CARGO_BIN_EXE_sting-scamcheck"))
        .arg(LIVE_JETTON)
        .output()
        .expect("the binary runs");
    let text = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "the check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.lines().count() > 3,
        "a bare answer is what this capability promises not to give: {text}"
    );
}

#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn a_gift_collection_answers_with_its_applicable_signals() {
    let output = Command::new(env!("CARGO_BIN_EXE_gift_trust_check"))
        .arg(LIVE_COLLECTION)
        .output()
        .expect("the binary runs");
    let text = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "the check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.lines().count() > 3,
        "one input, one structured answer across the applicable signals: {text}"
    );
}

/// An address that is not a jetton master is refused rather than scored.
///
/// The operational half: a check that scored anything it was handed would give a caller a number
/// about a contract that has none of the properties the number is about.
#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn an_address_that_is_not_a_jetton_is_refused() {
    let not_a_jetton = "0:0000000000000000000000000000000000000000000000000000000000000001";
    let output = Command::new(env!("CARGO_BIN_EXE_sting-scamcheck"))
        .arg(not_a_jetton)
        .output()
        .expect("the binary runs");

    assert!(
        !output.status.success(),
        "an address with no jetton master must not be scored"
    );
}
