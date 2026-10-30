//! Resolves TON "library reference" cells to their real, executable code —
//! a real, previously undocumented gap found while growing the dictionary
//! (`docs/domains/method-dictionary.md`):
//! TON lets many accounts share one copy of identical code by storing a
//! **library reference cell** (an "exotic" cell, TL-B `library_ref$_
//! library_hash:HashmapE` — TVM special cell type `2`) as the account's own
//! `code`, instead of the real code — the real, executable code lives once
//! in a masterchain-wide library table, keyed by hash. Every real, deployed
//! contract that opts into this space-saving convention (empirically
//! confirmed on a real USDT-on-TON governed jetton wallet — see
//! `corpus-abi/clean/func/SOURCE.md`) returns this tiny (~46-byte `BoC`)
//! pointer from `GET /api/v3/account`, not real code — feeding it straight
//! to the emulator (what every caller of this crate's `ContractSource` did
//! before this module existed) makes every single Tier 3 probe against it
//! come back `inconclusive`, not "confirmed absent" — the emulator can't
//! meaningfully execute a bare pointer cell as if it were a real dispatch
//! table.
//!
//! **Confirmed live** (2026-07-22): `toncenter.com`'s **v2**, not v3, API
//! exposes the real fix — `GET /api/v2/getLibraries?libraries=<hex hash>`
//! returns the real library code as a base64 `BoC`, ready to feed the
//! emulator directly. No equivalent v3 endpoint was found.

use std::time::Duration;

use sting_chain::{ChainClient, Endpoint, Network, Provider, RetryPolicy};
use tonlib_core::cell::BagOfCells;

const PROVIDER: &str = "toncenter v2 (getLibraries)";

/// Attempts before giving up on one `getLibraries` call, and the wait before
/// the second.
///
/// Found necessary live: the reprobe loop calls this once per unresolved code
/// with no pause between codes, and a transient hiccup fell back to the
/// unresolved pointer — the best-effort contract this module documents — so a
/// pass under-counted and the same pass run again picked up a few more. Short,
/// because this is one extra round trip per library cell rather than a page.
const RESOLVER_RETRY: RetryPolicy = RetryPolicy {
    max_attempts: 3,
    initial_backoff: Duration::from_millis(400),
    max_backoff: Duration::from_millis(1_600),
};

/// A TON library reference cell's real payload shape (confirmed against
/// `tonlib_core::cell::cell_type::CellType::validate_library`, which this
/// module can't call directly — that validation is `pub(crate)` to
/// `tonlib_core` — but the shape it enforces, `1 + 32` bytes, is public
/// knowledge from the TL-B schema itself: one type-tag byte (`2`, checked
/// via `Cell::is_library`), then the 32-byte library hash).
const LIBRARY_CELL_PAYLOAD_LEN: usize = 33;

/// Errors from [`LibraryResolver::resolve_if_library`].
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    /// `code_boc_base64` could not be parsed as a `BoC` at all, or wasn't a
    /// single-root `BoC` — genuinely malformed input, not a library-cell
    /// question.
    #[error("{PROVIDER}: not a valid single-root BoC: {0}")]
    MalformedBoc(String),
    /// The root cell claimed to be a library reference (`Cell::is_library`)
    /// but its payload wasn't the expected `1 + 32` bytes — inconsistent
    /// with the TL-B schema this module relies on; surfaced rather than
    /// silently ignored.
    #[error(
        "{PROVIDER}: library cell payload was {actual} bytes, expected {LIBRARY_CELL_PAYLOAD_LEN}"
    )]
    MalformedLibraryCell {
        /// The payload length actually found.
        actual: usize,
    },
    /// The request failed: it did not complete, it was refused past the
    /// retries, the status was one nobody reads, or the body was not the JSON
    /// this call expects. See `sting_chain::ChainError`.
    #[error(transparent)]
    Chain(#[from] sting_chain::ChainError),
    /// The library hash is not (or no longer) registered — a real,
    /// expected outcome (a library can be deleted), not necessarily a bug.
    #[error("{PROVIDER}: library {hash} not found")]
    NotFound {
        /// The hex-encoded library hash that was looked up.
        hash: String,
    },
}

#[derive(Debug, serde::Deserialize)]
struct GetLibrariesResponse {
    result: GetLibrariesResult,
}

#[derive(Debug, serde::Deserialize)]
struct GetLibrariesResult {
    result: Vec<LibraryEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct LibraryEntry {
    data: String,
}

/// Resolves TON library-reference cells through `toncenter`'s v2 API.
///
/// Separate from [`crate::ToncenterClient`], which speaks v3: the same
/// service, an older API, and the one capability v3 does not publish.
pub struct LibraryResolver {
    client: ChainClient,
}

impl LibraryResolver {
    /// A resolver against mainnet.
    #[must_use]
    pub fn new() -> Self {
        Self::for_network(Network::Mainnet)
    }

    /// A resolver against `network`, keyed from the environment.
    #[must_use]
    pub fn for_network(network: Network) -> Self {
        Self::over(Endpoint::public(Provider::ToncenterV2, network))
    }

    /// A resolver against another host — this module's own tests point it at
    /// a stub server.
    #[must_use]
    pub fn with_base_url(base_url: String) -> Self {
        Self::over(Endpoint::at(
            Provider::ToncenterV2,
            Network::Mainnet,
            base_url,
        ))
    }

    /// A resolver over `endpoint`.
    #[must_use]
    pub fn over(endpoint: Endpoint) -> Self {
        Self {
            client: ChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env())
                .with_retry_policy(RESOLVER_RETRY),
        }
    }

    /// If `code_boc_base64`'s single root cell is a TON library reference,
    /// resolves and returns the real code it points to. Returns `Ok(None)`
    /// — not an error — for the common case where `code_boc_base64` is
    /// already real code (not a library reference); callers substitute the
    /// `Some` value in place of the original code, otherwise use the
    /// original unchanged.
    ///
    /// # Errors
    /// See [`LibraryError`].
    pub fn resolve_if_library(
        &self,
        code_boc_base64: &str,
    ) -> Result<Option<String>, LibraryError> {
        let boc = BagOfCells::parse_base64(code_boc_base64)
            .map_err(|source| LibraryError::MalformedBoc(source.to_string()))?;
        let root = boc
            .single_root()
            .map_err(|source| LibraryError::MalformedBoc(source.to_string()))?;
        if !root.is_library() {
            return Ok(None);
        }
        let payload = root.data();
        if payload.len() != LIBRARY_CELL_PAYLOAD_LEN {
            return Err(LibraryError::MalformedLibraryCell {
                actual: payload.len(),
            });
        }
        // payload[0] is the exotic-cell type tag (already confirmed `2` by
        // `Cell::is_library` above) — the library hash is everything after it.
        let hash_hex = hex::encode(&payload[1..]);
        self.fetch_library(&hash_hex).map(Some)
    }

    /// One `getLibraries` call. The retrying is the client's, and what is
    /// left here is what the answer means: an empty result is a library that
    /// is not registered, which is an outcome rather than a failure — a
    /// library can be deleted.
    fn fetch_library(&self, hash_hex: &str) -> Result<String, LibraryError> {
        let body: GetLibrariesResponse = self
            .client
            .get_json("/getLibraries", &[("libraries", hash_hex)])?;
        body.result
            .result
            .into_iter()
            .next()
            .map(|entry| entry.data)
            .ok_or_else(|| LibraryError::NotFound {
                hash: hash_hex.to_string(),
            })
    }
}

impl Default for LibraryResolver {
    fn default() -> Self {
        Self::new()
    }
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

    use super::LibraryResolver;

    /// Same hand-rolled scripted-response server as
    /// `sting-corpus/tests/retry_against_mock_server.rs` and
    /// `sting-abi-coverage::walker`'s own tests.
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

    fn json_response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn status_response(status: u16) -> Vec<u8> {
        format!("HTTP/1.1 {status} Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes()
    }

    /// A real, unmodified library-reference-cell `BoC`, fetched live
    /// 2026-07-22 for a real, deployed governed-jetton-wallet address
    /// (`corpus-abi/clean/func/SOURCE.md`) — not synthesized, the exact
    /// bytes `toncenter.com/api/v3/account` returned.
    const REAL_LIBRARY_CELL_BOC: &str =
        "te6ccgEBAQEAIwAIQgKPRS16Tf10BmtoI2UXclntBXNENb52tf1L1divK3w9aA==";

    /// An ordinary (non-library, non-exotic) empty cell's `BoC`, to prove
    /// `resolve_if_library` leaves ordinary code alone — a real jetton-
    /// wallet-shaped `BoC` is overkill for this test, this module's own
    /// logic only looks at `Cell::is_library()`, not the code's actual
    /// content. Produced by `tonlib_core::cell::CellBuilder::new().build()`
    /// serialized to base64 — confirmed to parse as [`BagOfCells`] with
    /// `is_library() == false`.
    const ORDINARY_CELL_BOC: &str = "te6ccgEBAQEAAgAAAA==";

    #[test]
    fn resolve_if_library_returns_none_for_ordinary_code() {
        let resolver = LibraryResolver::with_base_url("http://127.0.0.1:1".to_string());
        let result = resolver
            .resolve_if_library(ORDINARY_CELL_BOC)
            .expect("ordinary code must not error");
        assert_eq!(
            result, None,
            "ordinary (non-library) code must be left alone, no network call attempted"
        );
    }

    #[test]
    fn resolve_if_library_fetches_and_returns_the_real_code_for_a_library_cell() {
        let real_code_b64 = "te6ccgECDwEAA9EAART/APSkE/S88sgLAQIBYgIDAvjQAdDTAwFxsI5IE18D";
        let response_body = format!(
            r#"{{"ok":true,"result":{{"@type":"smc.libraryResult","result":[{{"@type":"smc.libraryEntry","hash":"j0Utek39dAZraCNlF3JZ7QVzRDW+drX9S9XYryt8PWg=","data":"{real_code_b64}"}}]}},"@extra":"1:1:1"}}"#
        );
        let (base_url, server) = serve_scripted_responses(vec![json_response(&response_body)]);
        let resolver = LibraryResolver::with_base_url(base_url);

        let result = resolver
            .resolve_if_library(REAL_LIBRARY_CELL_BOC)
            .expect("resolves successfully");
        assert_eq!(result, Some(real_code_b64.to_string()));

        server.join().expect("server thread does not panic");
    }

    #[test]
    fn resolve_if_library_retries_a_transient_http_failure_and_then_succeeds() {
        let real_code_b64 = "te6ccgECDwEAA9EAART/APSkE/S88sgLAQIBYgIDAvjQAdDTAwFxsI5IE18D";
        let response_body = format!(
            r#"{{"ok":true,"result":{{"@type":"smc.libraryResult","result":[{{"@type":"smc.libraryEntry","hash":"j0Utek39dAZraCNlF3JZ7QVzRDW+drX9S9XYryt8PWg=","data":"{real_code_b64}"}}]}},"@extra":"1:1:1"}}"#
        );
        // First attempt hits a transient server error, second succeeds — a
        // single flaky response must not permanently mask a real library
        // cell as unresolved.
        let (base_url, server) =
            serve_scripted_responses(vec![status_response(503), json_response(&response_body)]);
        let resolver = LibraryResolver::with_base_url(base_url);

        let result = resolver
            .resolve_if_library(REAL_LIBRARY_CELL_BOC)
            .expect("recovers after the first attempt's transient failure");
        assert_eq!(result, Some(real_code_b64.to_string()));

        server.join().expect("server thread does not panic");
    }

    #[test]
    fn resolve_if_library_reports_not_found_for_an_empty_result() {
        let response_body =
            r#"{"ok":true,"result":{"@type":"smc.libraryResult","result":[]},"@extra":"1:1:1"}"#;
        let (base_url, server) = serve_scripted_responses(vec![json_response(response_body)]);
        let resolver = LibraryResolver::with_base_url(base_url);

        let result = resolver.resolve_if_library(REAL_LIBRARY_CELL_BOC);
        assert!(matches!(result, Err(super::LibraryError::NotFound { .. })));

        server.join().expect("server thread does not panic");
    }

    #[test]
    fn resolve_if_library_rejects_malformed_boc_before_any_network_call() {
        let resolver = LibraryResolver::with_base_url("http://127.0.0.1:1".to_string());
        let result = resolver.resolve_if_library("not-valid-base64-boc-at-all");
        assert!(matches!(result, Err(super::LibraryError::MalformedBoc(_))));
    }
}
