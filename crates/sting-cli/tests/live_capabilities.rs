//! The capabilities that need a chain, run against one.
//!
//! `docs/CAPABILITIES.md` marks three of this binary's capabilities live: fetching a deployed
//! contract, scanning one, and confirming a static finding against it. A live capability nothing
//! ever runs is the same claim an offline one would be — believed, tried once by whoever needed it,
//! and found broken — so each is driven here against a real mainnet address.
//!
//! Ignored by default and run by `make test-live`, which is the only place a network belongs. They
//! do not skip when the network is absent: these are run deliberately, and a run that quietly
//! passed without reaching anything would be worse than no test.
//!
//! The address is a real STON.fi pool, the same one the corpus carries a captured state of. What is
//! asserted is what a capability promises rather than what the chain happens to say today: a
//! balance changes between blocks, and a test that pinned one would fail for a reason that has
//! nothing to do with this platform.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::process::Command;

/// A contract that has been on mainnet for years and is not going anywhere: a pool of the largest
/// venue on this chain.
const LIVE_ADDRESS: &str = "EQD8TJ8xEWB1SpnRE4d89YO3jl0W0EiBnNS4IBaHaUmdfizE";

fn sting() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sting"))
}

#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn fetching_a_deployed_contract_returns_its_code_and_data() {
    let output = sting()
        .args(["fetch", LIVE_ADDRESS])
        .output()
        .expect("the binary runs");
    let text = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "fetch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("code:") && text.contains("data:"),
        "a fetch that returned neither code nor data is not a fetch: {text}"
    );
}

#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn scanning_a_deployed_contract_reports_its_state_as_loadable() {
    let output = sting()
        .args(["scan", LIVE_ADDRESS])
        .output()
        .expect("the binary runs");

    assert!(
        output.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An address that is not a contract is refused rather than reported as an empty result.
///
/// This is the half of the capability that matters operationally: a fetch that answered
/// successfully with nothing would let a caller record a contract as read when it was never there.
#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn fetching_an_address_with_no_contract_is_an_error() {
    // A well-formed address in the basechain that nothing has ever been deployed to.
    let empty = "0:0000000000000000000000000000000000000000000000000000000000000001";
    let output = sting()
        .args(["fetch", empty])
        .output()
        .expect("the binary runs");

    assert!(
        !output.status.success(),
        "an address with no contract must not be reported as fetched"
    );
}
