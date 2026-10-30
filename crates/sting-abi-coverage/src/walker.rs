//! `TransactionAddressWalker` — discovers real on-chain addresses by paging
//! `toncenter.com/api/v3/transactions` (`docs/domains/method-dictionary.md`
//! §6). Deliberately separate from
//! `sting_corpus::ContractSource` (fetch-by-**known**-address) — this is
//! **discovery**, a different job, and does not belong on that trait.
//!
//! # Confirmed live, not assumed (18.07.2026 → verified again 22.07.2026)
//!
//! Neither `toncenter` nor `tonapi`'s public APIs support filtering by code
//! hash or listing every account — `code_hash` only ever appears in a
//! *response*, never as a request filter. The only real discovery path is
//! paging `GET /api/v3/transactions`, which genuinely supports `limit`/
//! `offset` (unlike `/api/v3/actions`, unreliable under filters per
//! `docs/domains/abi-ir.md`'s own finding).
//!
//! **A further, better-than-planned finding made while implementing this
//! module**: every real transaction record already carries
//! `account_state_after.code_hash` — TON's own real cell hash, for free,
//! with no extra fetch per address. A live 200-transaction sample pulled
//! while building this module: 200 transactions → 83 distinct addresses →
//! only 21 distinct code hashes. This module therefore returns `(address,
//! code_hash)` pairs directly; a caller only needs to fetch a code's actual
//! `BoC` the *first* time a given `code_hash` is seen (`CoverageDb::
//! has_code_hash` before fetching) — not once per address. See
//! `crate::db`'s own module doc comment for the corresponding schema-level
//! deviation from the originally written plan (no local `sha2` hash at all).

use std::collections::HashSet;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use sting_chain::{ChainClient, Endpoint, Network, Provider, RetryPolicy};

const PROVIDER: &str = "toncenter v3 (transactions)";

/// What a page's request is retried on.
///
/// Four attempts rather than the default five, and the reason is what this
/// call is: a discovery pass makes thousands of them, and a page given up on
/// is retried by the next run rather than by this one.
pub(crate) const PAGE_RETRY: RetryPolicy = RetryPolicy {
    max_attempts: 4,
    initial_backoff: Duration::from_millis(500),
    max_backoff: Duration::from_secs(8),
};

/// One real `(address, code_hash)` pair discovered from a transaction
/// record's `account_state_after`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredAddress {
    /// The account's real address, raw form (`"<workchain>:<64 hex>"`), as
    /// `toncenter` itself returns it.
    pub address: String,
    /// The account's real code hash at the time of this transaction,
    /// hex-encoded (decoded from `toncenter`'s base64 representation).
    pub code_hash: String,
}

/// One page of discovery results.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransactionsPage {
    /// Newly discovered `(address, code_hash)` pairs, deduplicated by
    /// address *within this page* (a caller sweeping multiple pages must
    /// still dedupe across pages itself, e.g. via `CoverageDb`).
    pub discovered: Vec<DiscoveredAddress>,
    /// How many raw transaction records this page actually contained.
    /// `0` signals the stream is exhausted at this `offset` — `toncenter`
    /// returns fewer than the requested `limit` (down to zero) once there is
    /// nothing more to page through, the same convention
    /// `sting_corpus::ToncenterClient` relies on being well-behaved.
    pub record_count: usize,
}

/// Errors from [`TransactionAddressWalker::fetch_page`].
#[derive(Debug, thiserror::Error)]
pub enum WalkerError {
    /// The request failed: it did not complete, it was refused past the
    /// retries, the status was one nobody reads, or the body was not the JSON
    /// this call expects. See `sting_chain::ChainError`.
    #[error(transparent)]
    Chain(#[from] sting_chain::ChainError),
    /// A `code_hash` field was not valid base64.
    #[error("{0}: code_hash field was not valid base64: {1}")]
    Base64(&'static str, #[source] base64::DecodeError),
}

#[derive(Debug, serde::Deserialize)]
struct TransactionsResponse {
    transactions: Vec<TransactionRecord>,
}

#[derive(Debug, serde::Deserialize)]
struct TransactionRecord {
    account: String,
    account_state_after: Option<AccountState>,
}

#[derive(Debug, serde::Deserialize)]
struct AccountState {
    code_hash: Option<String>,
}

/// Pages `GET /api/v3/transactions`, newest-first, extracting distinct
/// `(address, code_hash)` pairs — the discovery half of a `sweep` run (the
/// fetch-actual-code half stays `sting_corpus::CorpusController`'s job,
/// per `docs/ARCHITECTURE.md`'s "one controller per concern").
pub struct TransactionAddressWalker {
    client: ChainClient,
}

impl TransactionAddressWalker {
    /// A walker against mainnet.
    #[must_use]
    pub fn new() -> Self {
        Self::for_network(Network::Mainnet)
    }

    /// A walker against `network`, keyed from the environment.
    #[must_use]
    pub fn for_network(network: Network) -> Self {
        Self::over(Endpoint::public(Provider::ToncenterV3, network))
    }

    /// A walker against another host — this module's own tests point it at a
    /// stub server.
    #[must_use]
    pub fn with_base_url(base_url: String) -> Self {
        Self::over(Endpoint::at(
            Provider::ToncenterV3,
            Network::Mainnet,
            base_url,
        ))
    }

    /// A walker over `endpoint`.
    #[must_use]
    pub fn over(endpoint: Endpoint) -> Self {
        Self {
            client: ChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env())
                .with_retry_policy(PAGE_RETRY),
        }
    }

    /// Fetches one page of up to `limit` transactions starting at `offset`
    /// (newest-first), returning distinct `(address, code_hash)` pairs.
    ///
    /// # Errors
    /// See [`WalkerError`].
    pub fn fetch_page(&self, limit: u32, offset: u32) -> Result<TransactionsPage, WalkerError> {
        let body: TransactionsResponse = self.client.get_json(
            "/transactions",
            &[
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
                ("sort", "desc"),
            ],
        )?;
        page_from_response(body)
    }
}

impl Default for TransactionAddressWalker {
    fn default() -> Self {
        Self::new()
    }
}

fn page_from_response(body: TransactionsResponse) -> Result<TransactionsPage, WalkerError> {
    let record_count = body.transactions.len();
    let mut seen_in_page = HashSet::new();
    let mut discovered = Vec::new();
    for tx in body.transactions {
        let Some(code_hash_b64) = tx.account_state_after.and_then(|s| s.code_hash) else {
            // A transaction with no post-state code hash (e.g. the account
            // was destroyed) carries nothing this module can act on — skip,
            // do not error the whole page over one such record.
            continue;
        };
        if !seen_in_page.insert(tx.account.clone()) {
            continue;
        }
        let code_hash_bytes = BASE64
            .decode(code_hash_b64.as_bytes())
            .map_err(|source| WalkerError::Base64(PROVIDER, source))?;
        discovered.push(DiscoveredAddress {
            address: tx.account,
            code_hash: hex::encode(code_hash_bytes),
        });
    }
    Ok(TransactionsPage {
        discovered,
        record_count,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::thread;

    use super::{TransactionAddressWalker, WalkerError};

    /// Same hand-rolled scripted-response server as
    /// `sting-corpus/tests/retry_against_mock_server.rs` — a real socket, no
    /// mock-HTTP-server dependency, for the same "tiny scripted exchange"
    /// reasoning that module's own doc comment gives.
    fn serve_scripted_responses(responses: Vec<Vec<u8>>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
        let port = listener.local_addr().expect("has a local addr").port();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accepts a connection");
                let mut buf = [0_u8; 4096];
                let mut total = 0;
                loop {
                    let n = stream
                        .read(&mut buf[total..])
                        .expect("reads from the socket");
                    total += n;
                    if n == 0 || buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                stream
                    .write_all(&response)
                    .expect("writes the scripted response");
                stream.flush().expect("flushes the response");
            }
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    fn json_response(status_line: &str, body: &str) -> Vec<u8> {
        format!(
            "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    /// A body shaped exactly like a real `toncenter.com/api/v3/transactions`
    /// response (confirmed via a live request while building this module) —
    /// two transactions sharing one address (must dedupe) and one
    /// transaction with no `account_state_after` (must be skipped, not
    /// error the page).
    fn real_shaped_body() -> String {
        r#"{"transactions":[
            {"account":"-1:aaaa","account_state_after":{"code_hash":"Y29kZS1oYXNoLWE="}},
            {"account":"-1:aaaa","account_state_after":{"code_hash":"Y29kZS1oYXNoLWE="}},
            {"account":"-1:bbbb","account_state_after":{"code_hash":"Y29kZS1oYXNoLWI="}},
            {"account":"-1:cccc","account_state_after":null}
        ]}"#
        .to_string()
    }

    #[test]
    fn fetch_page_dedupes_addresses_and_skips_records_with_no_post_state() {
        let (base_url, server) =
            serve_scripted_responses(vec![json_response("HTTP/1.1 200 OK", &real_shaped_body())]);
        let walker = TransactionAddressWalker::with_base_url(base_url);

        let page = walker.fetch_page(10, 0).expect("fetches successfully");
        assert_eq!(
            page.record_count, 4,
            "the raw transaction count, before dedup"
        );
        assert_eq!(
            page.discovered.len(),
            2,
            "two transactions for -1:aaaa must collapse into one discovered address"
        );
        assert_eq!(page.discovered[0].address, "-1:aaaa");
        assert_eq!(page.discovered[1].address, "-1:bbbb");
        assert_ne!(
            page.discovered[0].code_hash, page.discovered[1].code_hash,
            "distinct base64 code hashes must decode to distinct hex hashes"
        );

        server.join().expect("server thread does not panic");
    }

    #[test]
    fn fetch_page_reports_zero_records_as_the_end_of_the_stream() {
        let (base_url, server) = serve_scripted_responses(vec![json_response(
            "HTTP/1.1 200 OK",
            r#"{"transactions":[]}"#,
        )]);
        let walker = TransactionAddressWalker::with_base_url(base_url);

        let page = walker
            .fetch_page(10, 999_999)
            .expect("fetches successfully");
        assert_eq!(page.record_count, 0);
        assert!(page.discovered.is_empty());

        server.join().expect("server thread does not panic");
    }

    #[test]
    fn fetch_page_retries_past_a_429_and_succeeds_on_the_next_attempt() {
        let (base_url, server) = serve_scripted_responses(vec![
            json_response(
                "HTTP/1.1 429 Too Many Requests",
                r#"{"ok":false,"result":"Ratelimit exceed","code":429}"#,
            ),
            json_response("HTTP/1.1 200 OK", &real_shaped_body()),
        ]);
        let walker = TransactionAddressWalker::with_base_url(base_url);

        let page = walker.fetch_page(10, 0).expect("succeeds after one retry");
        assert_eq!(page.discovered.len(), 2);

        server.join().expect("server thread does not panic");
    }

    #[test]
    fn fetch_page_gives_up_after_exhausting_retries_against_a_permanently_rate_limited_server() {
        let responses = (0..super::PAGE_RETRY.max_attempts)
            .map(|_| {
                json_response(
                    "HTTP/1.1 429 Too Many Requests",
                    r#"{"ok":false,"result":"Ratelimit exceed","code":429}"#,
                )
            })
            .collect();
        let (base_url, server) = serve_scripted_responses(responses);
        let walker = TransactionAddressWalker::with_base_url(base_url);

        let result = walker.fetch_page(10, 0);
        assert!(
            matches!(
                result,
                Err(WalkerError::Chain(sting_chain::ChainError::RateLimited { attempts, .. }))
                    if attempts == super::PAGE_RETRY.max_attempts
            ),
            "expected a refusal after {} attempts, got {result:?}",
            super::PAGE_RETRY.max_attempts
        );

        server.join().expect("server thread does not panic");
    }
}
