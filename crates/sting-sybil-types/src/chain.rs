//! Logical time, hashes, amounts and finality: what the chain answers with, in the widths that
//! hold the answer.
//!
//! Each width here is a decision, not a default. An amount that overflows is money reported wrong,
//! and a width chosen by habit is a width nobody checked against the protocol.

/// Monotonic logical time of a transaction within one account.
pub type Lt = u64;

/// A transaction hash.
pub type TxHash = [u8; 32];

/// Nanotons. `u64` is enough for native TON: the total supply is about 5·10⁹ TON, that is
/// 5·10¹⁸ nanoton, against a `u64` maximum of roughly 1.8·10¹⁹ — an order of magnitude of room.
pub type Nanoton = u64;

/// A jetton amount. TEP-74 states `amount` as `VarUInteger 16`, so up to 120 bits: `u64` does not
/// hold an arbitrary jetton with large `decimals`, and the whole contour uses `u128` rather than
/// choosing per call site.
pub type JettonAmount = u128;

/// Where a fact stands in TON's finality lifecycle, which is what this chain has instead of a
/// reorg.
///
/// A fact read from a masterchain block that is not yet final can still be taken back, so a
/// clustering result computed over it is provisional; the state is carried rather than assumed so
/// that the caller decides what a provisional edge is worth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finality {
    /// Seen, and in no block yet.
    Pending,
    /// In a block, which is not yet signed off by the masterchain.
    Confirmed,
    /// In a block the masterchain has committed to.
    Finalized,
    /// Superseded: the block it was read from is not on the chain any more.
    Invalidated,
}
