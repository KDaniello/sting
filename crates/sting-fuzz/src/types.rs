//! Structured message-sequence input (`docs/domains/emulator.md`
//! §6.1) — `arbitrary`-derived so `cargo-fuzz`'s libFuzzer-driven byte
//! mutation produces *shaped* internal-message sequences instead of raw
//! bytes the emulator would reject before ever reaching interesting code
//! paths.

use arbitrary::Arbitrary;

/// A sequence of synthetic internal messages to send, one at a time, to a
/// single persistent contract instance — `fuzz_tx_sequence`'s input
/// (`docs/domains/emulator.md` item 2).
#[derive(Debug, Clone, Arbitrary)]
pub struct MsgSeq {
    /// The messages to send in order. Capped by [`crate::harness::MAX_MESSAGES_PER_SEQUENCE`]
    /// when actually run — an `arbitrary`-generated `Vec` has no inherent
    /// upper bound, and an unbounded sequence would make a single fuzz
    /// input's wall-clock cost unbounded too.
    pub messages: Vec<SyntheticMessage>,
}

/// One synthetic internal message — deliberately a flat, typed
/// `{op, amount, extra}` shape rather than the block doc's illustrative
/// nested `body_cells: Vec<CellStub>` sketch: `op`/`amount` are exactly the
/// two fields `crate::fixture::BalanceLedger`'s `onInternalMessage`
/// actually reads (see that module), and `extra` gives the fuzzer room to
/// explore malformed/trailing-data shapes without needing a second,
/// separately-mutated nested type. `fuzz_send_message`'s input
/// (`docs/domains/emulator.md` item 1).
#[derive(Debug, Clone, Arbitrary)]
pub struct SyntheticMessage {
    /// Read as the message body's first 32 bits (an op code, by Tolk/FunC
    /// convention).
    pub op: u32,
    /// Read as the message body's next 64 bits (an application-level
    /// amount — unrelated to the *attached* nanoton value, see
    /// [`crate::harness::ATTACHED_VALUE_NANOTONS`]).
    pub amount: u64,
    /// Extra trailing bytes appended to the body after `op`/`amount`,
    /// capped by [`crate::body::MAX_EXTRA_BYTES`] when actually built into
    /// a cell — explores "trailing data the contract doesn't expect"
    /// without needing a whole second message type.
    pub extra: Vec<u8>,
}
