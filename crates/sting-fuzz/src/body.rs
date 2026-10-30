//! Builds a real message-body `BoC` from a [`crate::types::SyntheticMessage`]
//! — a real `BoC`, serialised before the emulator is called, never a stand-in
//! (`docs/domains/emulator.md`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tonlib_core::cell::CellBuilder;

use crate::types::SyntheticMessage;

/// A single cell holds at most 1023 bits; `op` (32) + `amount` (64) already
/// use 96, leaving headroom for `extra` — capped well below the true limit
/// so a build failure from a pathological `extra` length is vanishingly
/// rare (and handled as "uninteresting input, skip" rather than a panic
/// either way — see [`build_body_boc`]).
pub const MAX_EXTRA_BYTES: usize = 100;

/// Builds the base64-`BoC` message body [`sting_emulator::TvmEmulator::send_internal_message`]
/// expects: `op` (32 bits) + `amount` (64 bits) + up to
/// [`MAX_EXTRA_BYTES`] of `extra`, matching exactly what
/// `BalanceLedger.tolk`'s `onInternalMessage` reads
/// (`in.body.loadUint(32)` then `in.body.loadUint(64)`) before touching
/// storage.
///
/// Returns `None` on a cell-construction failure (should not happen given
/// the byte cap above, but a fuzz target must treat this as "skip this
/// input", not a crash — building fixed-shape, in-range values failing is
/// itself uninteresting, not a finding).
#[must_use]
pub fn build_body_boc(msg: &SyntheticMessage) -> Option<String> {
    let mut builder = CellBuilder::new();
    builder.store_u32(32, msg.op).ok()?;
    builder.store_u64(64, msg.amount).ok()?;
    let extra_len = msg.extra.len().min(MAX_EXTRA_BYTES);
    builder.store_slice(&msg.extra[..extra_len]).ok()?;
    let cell = builder.build().ok()?;
    let bytes = tonlib_core::cell::BagOfCells::from_root(cell)
        .serialize(false)
        .ok()?;
    Some(BASE64.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::{SyntheticMessage, build_body_boc};

    #[test]
    fn builds_a_well_formed_boc_for_a_plain_message() {
        let msg = SyntheticMessage {
            op: 1,
            amount: 100,
            extra: vec![],
        };
        let boc = build_body_boc(&msg);
        assert!(boc.is_some());
    }

    #[test]
    fn truncates_extra_bytes_beyond_the_cap_instead_of_failing() {
        let msg = SyntheticMessage {
            op: 1,
            amount: 100,
            extra: vec![0xAB; super::MAX_EXTRA_BYTES * 4],
        };
        assert!(build_body_boc(&msg).is_some());
    }
}
