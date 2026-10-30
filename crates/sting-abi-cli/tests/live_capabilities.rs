//! The dictionary probe, run against a real deployed contract.
//!
//! It is the abi layer's answer where `extract` has nothing to work with: a deployed contract has
//! no source, and what can still be established is which of the method ids the dictionary knows
//! that bytecode answers. `docs/CAPABILITIES.md` marks it live, and this is what runs it.
//!
//! Ignored by default and run by `make test-live`. What is asserted is the shape of the answer
//! rather than a particular set of methods: a contract can be upgraded, and a test that pinned its
//! current interface would fail for a reason that has nothing to do with the probe.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::process::Command;

/// Tether's jetton master: a contract that answers the standard's get-methods and is not going
/// anywhere.
const LIVE_ADDRESS: &str = "EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs";

#[test]
#[ignore = "reaches the live chain: `make test-live`"]
fn probing_a_deployed_contract_names_the_methods_it_answers() {
    // The dictionary is named from the repository root, which is where the inventory's command runs
    // from; a test runs in its own crate directory, so the path is rebuilt rather than repeated.
    let dictionary = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus-abi/method-dictionary.json"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_abi-layer"))
        .args(["dictionary-probe", LIVE_ADDRESS, "--dictionary", dictionary])
        .output()
        .expect("the binary runs");
    let text = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "the probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("get_jetton_data"),
        "a jetton master answers the standard's own get-method, and the probe should say so: {text}"
    );
}
