//! Structured message generation, body-`BoC` building, and invariant
//! checking for `docs/domains/emulator.md` §9's coverage-guided fuzz targets —
//! the stable, plain-Rust half of them. The actual
//! libFuzzer entry points live in `fuzz/` (a separate, **not** a workspace
//! member, nightly-only `cargo-fuzz` project — see that directory's own
//! README): everything here builds and tests on stable Rust in the main
//! CI, so the invariants stay defined in code and checkable without nightly
//! or nightly-only `ASan` and libFuzzer instrumentation at all. `fuzz/`'s two
//! targets are thin wrappers calling straight into
//! [`harness::run_message_sequence`] — the same function this crate's own
//! regression tests call, so "the fuzzer found it" and "the checker still
//! catches it" are provably one code path, not two that could drift apart.

mod body;
mod fixture;
mod harness;
mod types;

pub use body::{MAX_EXTRA_BYTES, build_body_boc};
pub use fixture::{CODE_BOC_BASE64, DATA_BOC_BASE64};
pub use harness::{
    ATTACHED_VALUE_NANOTONS, GAS_LIMIT, InvariantViolation, MAX_MESSAGES_PER_SEQUENCE, RunOutcome,
    run_message_sequence,
};
pub use types::{MsgSeq, SyntheticMessage};
