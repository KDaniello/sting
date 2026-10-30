//! Turning a jetton wallet into its owner, by verification rather than by trust.
//!
//! A wallet contract will happily say who owns it. Anybody can deploy a contract that says the
//! same thing about somebody else's address, so the claim is only worth what the master says: the
//! master is asked which wallet it would have deployed for that owner, and the answer has to be
//! the wallet that was asked about.

use std::collections::HashSet;

use sting_sybil_types::{JettonWalletAddr, OwnerId, TonAddress};

/// Asking the chain, as a boundary rather than a client.
///
/// Deliberately synchronous: what is here is the algorithm, and it is worth being able to test it
/// without a network or a runtime. A production implementation wraps its own client around it.
pub trait JettonRpc {
    /// `get_wallet_data` on a jetton wallet, answering with its owner and its master.
    fn wallet_data(&self, jetton_wallet: JettonWalletAddr) -> Option<(TonAddress, TonAddress)>;

    /// `get_wallet_address` on a jetton master, answering with the wallet it would deploy for
    /// that owner.
    fn expected_wallet_address(
        &self,
        master: TonAddress,
        owner: TonAddress,
    ) -> Option<JettonWalletAddr>;
}

/// The jetton masters whose wallets are worth resolving at all.
#[derive(Default)]
pub struct MasterAllowlist(HashSet<TonAddress>);

impl MasterAllowlist {
    /// Holds `masters` and nothing else.
    #[must_use]
    pub fn new(masters: impl IntoIterator<Item = TonAddress>) -> Self {
        Self(masters.into_iter().collect())
    }

    /// Whether this master is one of them.
    #[must_use]
    pub fn contains(&self, master: &TonAddress) -> bool {
        self.0.contains(master)
    }
}

/// The owner of `jetton_wallet`, when the wallet is authentic against an allowed master.
///
/// Four steps, and the answer is `None` the moment one of them does not hold: ask the wallet who
/// owns it, check the master it names is one worth trusting, ask that master which wallet belongs
/// to that owner, and compare it with the wallet in hand.
#[must_use]
pub fn resolve_and_verify_owner(
    rpc: &impl JettonRpc,
    jetton_wallet: JettonWalletAddr,
    allowlist: &MasterAllowlist,
) -> Option<OwnerId> {
    resolve_and_verify_owner_with_master(rpc, jetton_wallet, allowlist).map(|(owner, _)| owner)
}

/// The same, also handing back the master.
///
/// The pipeline needs it to build an event, and the public entry point above is what callers
/// already depend on — so this is a second door into one algorithm rather than a second
/// algorithm.
pub(crate) fn resolve_and_verify_owner_with_master(
    rpc: &impl JettonRpc,
    jetton_wallet: JettonWalletAddr,
    allowlist: &MasterAllowlist,
) -> Option<(OwnerId, TonAddress)> {
    let (owner, master) = rpc.wallet_data(jetton_wallet)?;
    if !allowlist.contains(&master) {
        return None;
    }
    let expected = rpc.expected_wallet_address(master, owner)?;
    if expected != jetton_wallet {
        // The wallet is not the one this master would have deployed, so the claim of ownership is
        // somebody else's contract talking.
        return None;
    }
    Some((OwnerId(owner), master))
}
