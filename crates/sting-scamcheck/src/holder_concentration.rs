//! Holder-concentration signal (`docs/domains/scamcheck.md`) — the one vector
//! with no core reuse at all: on-chain get methods
//! cannot enumerate a jetton's holders, only an indexer can. Uses a
//! minimal, independent `ureq`-based client (the same `ureq` centralized in
//! root `Cargo.toml`'s `[workspace.dependencies]` that `sting-corpus`
//! already used — moved there on review, per `docs/ARCHITECTURE.md`'s own
//! single-source-of-truth policy for any dependency shared by 2+ crates)
//! rather than extending `sting_corpus::ToncenterClient` — that client's
//! whole shape (`fetch(address) -> ContractCode`) exists for resolving one
//! contract's code+data by address, not for listing a jetton's wallets;
//! bolting an unrelated endpoint onto it would be scope creep on a core
//! crate for a product-specific need, the same reasoning already applied
//! against extending `sting-abi-*`'s heuristics.
//!
//! **`sort=desc` is real, verified against a live response, not assumed**:
//! `toncenter.com/api/v3/jetton/wallets?jetton_address=...&sort=desc`
//! returns wallets ordered by `balance` descending (checked against a real
//! high-volume jetton — USD₮ — comparing `sort=asc` vs `sort=desc` results
//! on 2026-07-23; `asc` returned all-zero balances first, `desc` returned
//! the largest balances first) — this lets the server do the sorting, no
//! need to page through every holder and sort client-side. No runtime
//! check re-validates this ordering (would need re-sorting anyway, which
//! defeats the point) — a silent regression in toncenter's own `sort`
//! behavior would go undetected here.
//!
//! **Known limitation, stated explicitly, not silently absent**: no
//! retry/backoff or explicit `429` handling, unlike
//! `sting_corpus::ToncenterClient` (same host, same free-tier rate limit,
//! a full `RetryPolicy` — see that module's own doc comment on why the
//! limit is a real, exercised path, not a hypothetical). Being refused is
//! `sting-chain`'s business, and this module inherits the one backoff every
//! call in this tree waits out.

use sting_chain::{ChainClient, Endpoint, Network, Provider};
use thiserror::Error;

/// Where this signal is read from. A jetton's holders are a mainnet fact, and
/// which host answers for a network is the chain client's table rather than a
/// constant here.
fn default_base_url() -> String {
    Endpoint::public(Provider::ToncenterV3, Network::Mainnet).base_url
}

/// The client one call goes out through: paced, retried and keyed like every
/// other call this platform makes.
fn client_at(base_url: &str) -> ChainClient {
    ChainClient::keyed(
        Endpoint::at(Provider::ToncenterV3, Network::Mainnet, base_url),
        &sting_chain::ApiKeys::from_env(),
    )
}

/// Errors fetching or interpreting a jetton's top holders.
#[derive(Debug, Error)]
pub enum HolderConcentrationError {
    /// The request failed: it did not complete, it was refused past the
    /// retries, the status was one nobody reads, or the body was not the JSON
    /// this call expects. See `sting_chain::ChainError`.
    #[error(transparent)]
    Chain(#[from] sting_chain::ChainError),
    /// A `balance` field was not a valid non-negative integer string.
    #[error("failed to parse balance {value:?}: {source}")]
    Balance {
        /// The raw string value that failed to parse.
        value: String,
        /// The underlying parse failure.
        #[source]
        source: std::num::ParseIntError,
    },
    /// An `address`/`owner` field was not a parseable TON address.
    #[error("failed to parse {field} {value:?}: {source}")]
    Address {
        /// Which field failed (`"address"` or `"owner"`).
        field: String,
        /// The raw string value that failed to parse.
        value: String,
        /// The underlying parse failure.
        #[source]
        source: tonlib_core::TonAddressParseError,
    },
}

#[derive(Debug, serde::Deserialize)]
struct WalletsResponse {
    jetton_wallets: Vec<WalletEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct WalletEntry {
    address: String,
    owner: String,
    balance: String,
}

/// One real jetton holder — used by [`crate::honeypot`] to probe an actual
/// deployed wallet instead of a hand-built synthetic one (see that
/// module's doc comment for why: guessing at a non-standard wallet's
/// internal storage layout is exactly the failure mode a real holder's
/// real on-chain bytes sidesteps entirely).
#[derive(Debug, Clone)]
pub struct Holder {
    /// The jetton wallet's own address.
    pub wallet_address: tonlib_core::TonAddress,
    /// The wallet's owner — who must be the message `src` for a transfer
    /// to pass the standard sender check.
    pub owner: tonlib_core::TonAddress,
    /// This holder's jetton balance.
    pub balance: u128,
}

fn parse_ton_address(
    field: &str,
    value: &str,
) -> Result<tonlib_core::TonAddress, HolderConcentrationError> {
    value
        .parse()
        .map_err(|source| HolderConcentrationError::Address {
            field: field.to_string(),
            value: value.to_string(),
            source,
        })
}

/// The result of summing the top `holders_fetched` wallets' balances for a
/// jetton, against its total supply.
#[derive(Debug, Clone)]
pub struct HolderConcentrationSignal {
    /// How many top holders were actually fetched and summed (may be less
    /// than requested if the jetton has fewer distinct holders).
    pub holders_fetched: usize,
    /// Sum of the top holders' balances, same base units as `total_supply`.
    pub top_holders_balance_sum: u128,
    /// The jetton's total supply (same base units), supplied by the
    /// caller — typically from [`crate::mint_rights`]'s own
    /// `get_jetton_data()` call, not re-fetched here.
    pub total_supply: u128,
}

impl HolderConcentrationSignal {
    /// The top holders' combined share of total supply, as a fraction in
    /// `[0.0, 1.0]` (or slightly above, if `total_supply` under-counts
    /// relative to on-chain reality — surfaced as-is, not clamped, so a
    /// caller can notice the discrepancy rather than have it hidden).
    /// Returns `None` if `total_supply` is zero (nothing to divide by).
    #[must_use]
    pub fn top_holders_share(&self) -> Option<f64> {
        if self.total_supply == 0 {
            return None;
        }
        // Deliberate `as` cast to `f64` for a human-facing ratio only —
        // this signal is a coarse concentration indicator, not a ledger
        // balance; precision loss on values this large is immaterial to
        // that use, unlike `mint_rights`/`ston_fi`'s raw integer amounts,
        // which are kept as exact `i64`s throughout.
        #[allow(
            clippy::cast_precision_loss,
            reason = "human-facing ratio, not an exact accounting value"
        )]
        let (sum, total) = (
            self.top_holders_balance_sum as f64,
            self.total_supply as f64,
        );
        Some(sum / total)
    }
}

/// Fetches the top `top_n` holders of `jetton_master` (by balance,
/// descending), with each holder's own wallet/owner address — used by
/// [`crate::honeypot`] to probe real deployed wallets.
///
/// # Errors
/// See [`HolderConcentrationError`].
pub fn fetch_top_holders(
    jetton_master: &tonlib_core::TonAddress,
    top_n: usize,
) -> Result<Vec<Holder>, HolderConcentrationError> {
    fetch_top_holders_from(&default_base_url(), jetton_master, top_n)
}

/// [`fetch_top_holders`], against `base_url` instead of the real
/// `toncenter.com` (e.g. a local mock server in tests).
///
/// # Errors
/// See [`HolderConcentrationError`].
pub fn fetch_top_holders_from(
    base_url: &str,
    jetton_master: &tonlib_core::TonAddress,
    top_n: usize,
) -> Result<Vec<Holder>, HolderConcentrationError> {
    let body: WalletsResponse = client_at(base_url).get_json(
        "/jetton/wallets",
        &[
            ("jetton_address", &jetton_master.to_hex()),
            ("limit", &top_n.to_string()),
            ("sort", "desc"),
        ],
    )?;

    body.jetton_wallets
        .into_iter()
        .map(|wallet| {
            let balance = wallet.balance.parse::<u128>().map_err(|source| {
                HolderConcentrationError::Balance {
                    value: wallet.balance.clone(),
                    source,
                }
            })?;
            let wallet_address = parse_ton_address("address", &wallet.address)?;
            let owner = parse_ton_address("owner", &wallet.owner)?;
            Ok(Holder {
                wallet_address,
                owner,
                balance,
            })
        })
        .collect()
}

/// Fetches the top `top_n` holders of `jetton_master` (by balance,
/// descending) and sums their balances.
///
/// # Errors
/// See [`HolderConcentrationError`].
pub fn fetch_top_holders_concentration(
    jetton_master: &tonlib_core::TonAddress,
    top_n: usize,
    total_supply: u128,
) -> Result<HolderConcentrationSignal, HolderConcentrationError> {
    fetch_top_holders_concentration_from(&default_base_url(), jetton_master, top_n, total_supply)
}

/// [`fetch_top_holders_concentration`], against `base_url` instead of the
/// real `toncenter.com` (e.g. a local mock server in tests).
///
/// # Errors
/// See [`HolderConcentrationError`].
pub fn fetch_top_holders_concentration_from(
    base_url: &str,
    jetton_master: &tonlib_core::TonAddress,
    top_n: usize,
    total_supply: u128,
) -> Result<HolderConcentrationSignal, HolderConcentrationError> {
    let holders = fetch_top_holders_from(base_url, jetton_master, top_n)?;
    let holders_fetched = holders.len();
    // `saturating_add`, not checked/wrapping: summing at most `top_n` real
    // jetton balances can never realistically approach `u128::MAX`
    // (~3.4×10^38) — saturating here is a no-op in practice, chosen over
    // `checked_add` only to avoid an unreachable error variant nothing
    // could ever construct a test for.
    let sum = holders
        .iter()
        .fold(0_u128, |acc, holder| acc.saturating_add(holder.balance));

    Ok(HolderConcentrationSignal {
        holders_fetched,
        top_holders_balance_sum: sum,
        total_supply,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::HolderConcentrationSignal;

    #[test]
    fn share_divides_sum_by_total_supply() {
        let signal = HolderConcentrationSignal {
            holders_fetched: 3,
            top_holders_balance_sum: 60,
            total_supply: 100,
        };
        assert!((signal.top_holders_share().expect("non-zero total_supply") - 0.6).abs() < 1e-9);
    }

    #[test]
    fn share_is_none_for_zero_total_supply() {
        let signal = HolderConcentrationSignal {
            holders_fetched: 0,
            top_holders_balance_sum: 0,
            total_supply: 0,
        };
        assert!(signal.top_holders_share().is_none());
    }
}
