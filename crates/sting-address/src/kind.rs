//! What sits at an address, and the two kinds that must never be confused.

use crate::raw::TonAddress;

/// What is deployed at an address, as far as the caller has established.
///
/// A classification a caller arrived at, not one this crate performs: deciding
/// it means reading a code hash off the chain, and this crate reaches nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccountKind {
    /// A known wallet code hash — the only kind that is a valid owner.
    Wallet,
    /// A jetton wallet: reached only through resolution, never taken as an
    /// owner.
    JettonWallet,
    /// A jetton master.
    JettonMaster,
    /// An NFT item.
    NftItem,
    /// An NFT collection.
    NftCollection,
    /// An exchange's hot wallet, from an allowlist.
    Exchange,
    /// A bridge, from an allowlist.
    Bridge,
    /// Any other contract — a pool, a router — excluded from a graph by
    /// default.
    Contract,
    /// Not deployed, or not classified yet.
    Unknown,
}

/// An address established to be a wallet: the one kind that is a graph node.
///
/// **No conversion to or from [`JettonWalletAddr`] may ever be written.** The
/// absence of that conversion is the entire protection against reading a
/// jetton wallet as its owner, and adding one quietly removes it — a graph
/// built on jetton wallets rather than owners is a graph of the wrong thing
/// that looks exactly like the right one. `tests/compile_fail/` is what holds
/// the rule: it fails to compile, on purpose.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize)]
pub struct OwnerId(pub TonAddress);

/// The address of a jetton wallet contract.
///
/// Never a graph node: it goes into resolution, which answers with the owner.
/// The warning on [`OwnerId`] holds here symmetrically — no conversion between
/// the two, in either direction.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize)]
pub struct JettonWalletAddr(pub TonAddress);

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{AccountKind, JettonWalletAddr, OwnerId};
    use crate::raw::TonAddress;

    #[test]
    fn the_two_kinds_of_address_are_distinct_types_over_the_same_address() {
        let address = TonAddress::new(0, [0x77; 32]);
        let owner = OwnerId(address);
        let wallet = JettonWalletAddr(address);
        // Same address underneath, and no way from one wrapper to the other.
        assert_eq!(owner.0, wallet.0);
    }

    #[test]
    fn a_kind_is_a_value_rather_than_a_lookup() {
        assert_ne!(AccountKind::Wallet, AccountKind::JettonWallet);
        assert_eq!(AccountKind::Unknown, AccountKind::Unknown);
    }
}
