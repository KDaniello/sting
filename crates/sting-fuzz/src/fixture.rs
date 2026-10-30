//! The deliberately vulnerable contract the harness is validated
//! against (`docs/domains/emulator.md`,
//! `corpus/fixtures/fuzz/SOURCE.md`) — embedded at compile time so every
//! consumer (unit tests here, the `cargo-fuzz` targets in `fuzz/`) gets the
//! exact same bytes without needing to resolve a runtime file path (a
//! libFuzzer binary can be invoked from any working directory).

/// Compiled code cell (base64 `BoC`) for `corpus/fixtures/fuzz/BalanceLedger.tolk`.
pub const CODE_BOC_BASE64: &str =
    include_str!("../../../corpus/fixtures/fuzz/BalanceLedger.code.boc.base64");

/// Initial data cell (base64 `BoC`, `balance = 0`) for the same contract.
pub const DATA_BOC_BASE64: &str =
    include_str!("../../../corpus/fixtures/fuzz/BalanceLedger.data.boc.base64");
