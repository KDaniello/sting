//! Wallet/relayer behavioral-reputation signal
//! (`docs/domains/scamcheck.md`) — same
//! reasoning as `holder_concentration.rs` for using an independent `ureq`
//! client rather than extending `sting_corpus::TonapiClient` (that client
//! resolves one address's code+data, not event history — bolting an
//! unrelated endpoint onto a core crate for this product's own need would
//! be the same scope creep already avoided there).
//!
//! Real response shape confirmed by a live request during this module's own
//! implementation (`GET tonapi.io/v2/accounts/<address>/events?limit=N`,
//! 2026-07-23), not assumed from documentation: each event carries the
//! account's own `is_scam` flag (tonapi's own classification, free to read)
//! plus a list of `actions`, each naming an `executor`/`contract` (and other
//! per-action-type address fields) — this module reads `is_scam` directly
//! and aggregates the distinct addresses seen across actions as a
//! counterparty-diversity proxy.
//!
//! **Scope, stated explicitly — this is account age *within the fetched
//! page*, not true account age since genesis**: computing genuine
//! first-ever-activity age would need paginating every event back to the
//! account's creation, which for an active relayer bot handling thousands
//! of trades could be a very large number of requests — out of scope for
//! tonight's first version, a conscious decision (same as
//! `holder_concentration.rs`'s own documented no-retry/429-handling
//! decision), not a silently narrower result presented as the real thing.

use std::collections::HashSet;

use serde::Deserialize;
use sting_chain::{ChainClient, Endpoint, Network, Provider};
use thiserror::Error;

/// Where this signal is read from — the second service, whose rate limit is
/// its own, which is what makes it useful when the first one's is spent.
fn default_base_url() -> String {
    Endpoint::public(Provider::TonapiV2, Network::Mainnet).base_url
}

/// The client one call goes out through.
fn client_at(base_url: &str) -> ChainClient {
    ChainClient::keyed(
        Endpoint::at(Provider::TonapiV2, Network::Mainnet, base_url),
        &sting_chain::ApiKeys::from_env(),
    )
}

/// Errors fetching or interpreting an account's recent event history.
#[derive(Debug, Error)]
pub enum WalletReputationError {
    /// The request failed: it did not complete, it was refused past the
    /// retries, the status was one nobody reads, or the body was not the JSON
    /// this call expects. See `sting_chain::ChainError`.
    #[error(transparent)]
    Chain(#[from] sting_chain::ChainError),
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
struct Event {
    account: EventAccount,
    timestamp: i64,
    #[serde(default)]
    actions: Vec<Action>,
}

#[derive(Debug, Deserialize)]
struct EventAccount {
    #[serde(default)]
    is_scam: bool,
}

#[derive(Debug, Deserialize)]
struct Action {
    #[serde(rename = "SmartContractExec", default)]
    smart_contract_exec: Option<SmartContractExec>,
}

#[derive(Debug, Deserialize)]
struct SmartContractExec {
    executor: ActionAddress,
    contract: ActionAddress,
}

#[derive(Debug, Deserialize)]
struct ActionAddress {
    address: String,
}

/// The behavioral-reputation signal for one address, aggregated over the
/// most recent `limit` events fetched — see the module doc comment for why
/// this is page-scoped, not full-history.
#[derive(Debug, Clone)]
pub struct WalletReputationSignal {
    /// How many events were actually fetched (may be less than requested if
    /// the account has fewer).
    pub events_fetched: usize,
    /// Distinct counterparty addresses (`executor`/`contract` from
    /// `SmartContractExec` actions) seen across the fetched events —
    /// excludes the queried address itself.
    pub unique_counterparties: usize,
    /// The oldest `timestamp` among the fetched events — **not** true
    /// account age, see the module doc comment.
    pub oldest_timestamp_in_page: Option<i64>,
    /// The newest `timestamp` among the fetched events.
    pub newest_timestamp_in_page: Option<i64>,
    /// Whether any fetched event flagged this account `is_scam: true` —
    /// tonapi's own classification, read directly, not derived.
    pub any_event_flagged_scam: bool,
}

/// Fetches and aggregates `address`'s most recent `limit` events.
///
/// # Errors
/// See [`WalletReputationError`].
pub fn fetch_wallet_reputation(
    address: &tonlib_core::TonAddress,
    limit: usize,
) -> Result<WalletReputationSignal, WalletReputationError> {
    fetch_wallet_reputation_from(&default_base_url(), address, limit)
}

/// [`fetch_wallet_reputation`], against `base_url` instead of the real
/// `tonapi.io` (e.g. a local mock server in tests).
///
/// # Errors
/// See [`WalletReputationError`].
pub fn fetch_wallet_reputation_from(
    base_url: &str,
    address: &tonlib_core::TonAddress,
    limit: usize,
) -> Result<WalletReputationSignal, WalletReputationError> {
    let path = format!("/accounts/{}/events", address.to_hex());
    let body: EventsResponse =
        client_at(base_url).get_json(&path, &[("limit", &limit.to_string())])?;

    let queried_hex = address.to_hex();
    let mut counterparties: HashSet<String> = HashSet::new();
    let mut oldest: Option<i64> = None;
    let mut newest: Option<i64> = None;
    let mut any_event_flagged_scam = false;

    for event in &body.events {
        if event.account.is_scam {
            any_event_flagged_scam = true;
        }
        oldest = Some(oldest.map_or(event.timestamp, |o: i64| o.min(event.timestamp)));
        newest = Some(newest.map_or(event.timestamp, |n: i64| n.max(event.timestamp)));

        for action in &event.actions {
            if let Some(exec) = &action.smart_contract_exec {
                for addr in [&exec.executor.address, &exec.contract.address] {
                    let normalized = addr.to_lowercase();
                    if normalized != queried_hex.to_lowercase() {
                        counterparties.insert(normalized);
                    }
                }
            }
        }
    }

    Ok(WalletReputationSignal {
        events_fetched: body.events.len(),
        unique_counterparties: counterparties.len(),
        oldest_timestamp_in_page: oldest,
        newest_timestamp_in_page: newest,
        any_event_flagged_scam,
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
    use super::WalletReputationSignal;

    #[test]
    fn signal_fields_are_constructible_and_readable() {
        // A lightweight smoke test of the struct shape itself — the real
        // parsing path is exercised by `tests/wallet_reputation.rs` against
        // a live response (network-gated, see that file).
        let signal = WalletReputationSignal {
            events_fetched: 3,
            unique_counterparties: 2,
            oldest_timestamp_in_page: Some(100),
            newest_timestamp_in_page: Some(300),
            any_event_flagged_scam: false,
        };
        assert_eq!(signal.events_fetched, 3);
        assert!(!signal.any_event_flagged_scam);
    }
}
