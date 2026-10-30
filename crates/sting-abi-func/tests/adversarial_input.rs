//! Panic-resilience against malformed `FunC` source — `docs/abi-layer/
//! ARCHITECTURE.md` §6 requires `FuncExtractor` to be "panic-resilient
//! against any `.fc` file from the internet," but until now nothing checked
//! that beyond `parse_real_func_samples.rs`'s positive-corpus smoke test
//! (valid fixtures only, asserting 0 error nodes — never exercises the
//! panic-resilience requirement at all). Real gap found by review
//! .
//!
//! Not wired into `sting-fuzz` (that crate is deliberately scoped to the
//! FFI/`unsafe` boundary only — `docs/TESTING.md`/§7
//! — `FuncExtractor` has neither): a `proptest`-driven mutation test here
//! runs in ordinary `cargo test`, no nightly toolchain or `cargo-fuzz`
//! needed, same pattern `sting-abi-service/tests/adversarial.rs` already
//! uses for its own HTTP-path adversarial coverage.
//!
//! Strategy: take a real, known-good `.fc` fixture (`tests/fixtures/`,
//! provenance `tests/fixtures/SOURCE.md`) and apply one random splice
//! mutation (delete a random-length span at a random position, insert
//! random bytes there instead) — not fully random garbage from nothing,
//! since that mostly just exercises "reject immediately," not the more
//! interesting "look almost like real `FunC` but broken partway through"
//! shapes an adversarial submitter would actually produce.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::Path;

use proptest::prelude::*;
use sting_abi_core::Extractor;
use sting_abi_func::FuncExtractor;

fn real_fixture_sources() -> Vec<String> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&fixtures_dir).expect("fixtures directory must exist") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("fc") {
            sources.push(std::fs::read_to_string(&path).expect("readable real fixture"));
        }
    }
    assert!(
        !sources.is_empty(),
        "need at least one real .fc fixture under tests/fixtures/ to mutate"
    );
    sources
}

/// Deletes a `delete_frac`-sized span starting at `position_frac` (both
/// fractions of the seed's byte length, clamped into range) and splices
/// `insert` in its place — byte-level, not UTF-8-aware on purpose, so the
/// result may not even be valid UTF-8 (a real adversarial upload could be
/// anything; `FuncExtractor::extract` reading it via `read_to_string` must
/// fail cleanly, not panic, on that case too).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "real fixtures are at most a few KB, nowhere near where f64<->usize round-tripping \
              loses meaningful precision; both fractions are clamped to [0.0, 1.0] first, so the \
              product is always non-negative and bounded by `len`/`remaining`, both re-clamped \
              via `.min()` right after the cast as a second belt-and-braces bound"
)]
fn splice_mutation(seed: &str, position_frac: f64, delete_frac: f64, insert: &[u8]) -> Vec<u8> {
    let bytes = seed.as_bytes();
    let len = bytes.len();
    let position = ((position_frac.clamp(0.0, 1.0) * len as f64) as usize).min(len);
    let remaining = len - position;
    let delete_len = ((delete_frac.clamp(0.0, 1.0) * remaining as f64) as usize).min(remaining);

    let mut mutated = Vec::with_capacity(len + insert.len());
    mutated.extend_from_slice(&bytes[..position]);
    mutated.extend_from_slice(insert);
    mutated.extend_from_slice(&bytes[position + delete_len..]);
    mutated
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Regression target for `docs/ARCHITECTURE.md` §6's panic-resilience
    /// requirement: no single-splice mutation of a real fixture may ever
    /// panic `FuncExtractor::extract` — an `Err` (or a degraded `Ok` with
    /// `Unknown`/missing shapes) is the only acceptable outcome for
    /// malformed input.
    #[test]
    fn func_extractor_never_panics_on_a_mutated_real_fixture(
        seed_index in 0usize..64,
        position_frac in 0.0f64..1.0,
        delete_frac in 0.0f64..1.0,
        insert in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let sources = real_fixture_sources();
        let seed = &sources[seed_index % sources.len()];
        let mutated = splice_mutation(seed, position_frac, delete_frac, &insert);

        let scratch = tempfile::tempdir().expect("scratch dir");
        let entrypoint = Path::new("mutated.fc");
        std::fs::write(scratch.path().join(entrypoint), &mutated).expect("writable scratch file");

        let extractor = FuncExtractor;
        let project_dir = scratch.path().to_path_buf();
        let entrypoint = entrypoint.to_path_buf();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extractor.extract(&project_dir, &entrypoint)
        }));

        prop_assert!(
            result.is_ok(),
            "FuncExtractor::extract panicked on a mutated real fixture — this is exactly the \
             panic-resilience requirement docs/ARCHITECTURE.md §6 states and, until this test, \
             nothing checked"
        );
    }
}
