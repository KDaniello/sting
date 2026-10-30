//! The core: one call, paced, retried, authenticated, and answered as JSON.

use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::config::Config;

use std::sync::Arc;

use crate::credentials::{ApiKey, ApiKeys};
use crate::endpoint::{Endpoint, Network, Provider};
use crate::error::ChainError;
use crate::limiter::{self, RateLimiter};
use crate::retry::{Attempt, DEFAULT_CALL_TIMEOUT, RetryPolicy, with_retries};
use crate::retry_after;

/// HTTP status for a rate limit, the one non-success this client reads rather
/// than reports.
const RATE_LIMITED: u16 = 429;

/// Where the server's own failures start. Below it the request is what is
/// wrong; from it the service is.
const SERVER_ERROR: u16 = 500;

/// The query of one request: parameter names and values, in the order a
/// caller wrote them.
///
/// A slice of pairs rather than a map, because the order of a query is what a
/// person compares against a provider's own documentation when a call answers
/// something unexpected.
pub type Query<'a> = &'a [(&'a str, &'a str)];

/// One service, reached at one pace, with one retry policy.
///
/// Cloning is not offered on purpose: two clients are two paces, and the
/// point of this crate is that there is one. A caller needing the client in
/// several places shares it by reference.
pub struct ChainClient {
    endpoint: Endpoint,
    agent: Agent,
    key: Option<ApiKey>,
    limiter: Arc<RateLimiter>,
    retry: RetryPolicy,
}

impl ChainClient {
    /// A client for the provider's own host on `network`, keyed from the
    /// environment and paced at the rate that key buys.
    ///
    /// Without a key the pace drops to one request a second, which is what the
    /// public tier answers: asking for ten without a key does not make it ten,
    /// it makes every tenth call a refusal.
    #[must_use]
    pub fn public(provider: Provider, network: Network) -> Self {
        Self::keyed(Endpoint::public(provider, network), &ApiKeys::from_env())
    }

    /// A client for `endpoint`, taking its key from `keys`.
    ///
    /// The pace comes from the limiter registry rather than from this client:
    /// a limiter per client is a limiter per call for whoever builds one per
    /// call, and two limiters against one service admit two calls at once.
    #[must_use]
    pub fn keyed(endpoint: Endpoint, keys: &ApiKeys) -> Self {
        let key = keys.get(endpoint.provider, endpoint.network).cloned();
        // The keyed tier is ten a second and the public one is about one.
        // Pacing at the tier the caller actually has is the difference between
        // waiting and being refused.
        let per_second = if key.is_some() {
            crate::limiter::DEFAULT_REQUESTS_PER_SECOND
        } else {
            1
        };
        Self {
            limiter: limiter::shared(
                endpoint.provider,
                endpoint.network,
                &endpoint.base_url,
                per_second,
            ),
            endpoint,
            agent: agent(),
            key,
            retry: RetryPolicy::default(),
        }
    }

    /// A client with no key and no pacing — a stub server, where a pace only
    /// makes a test slower.
    #[must_use]
    pub fn unlimited(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            agent: agent(),
            key: None,
            limiter: Arc::new(RateLimiter::unlimited()),
            retry: RetryPolicy::default(),
        }
    }

    /// The same client with another retry policy.
    #[must_use]
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The same client at a pace of its own — a stub server, or a caller that
    /// knows its quota is not the default one.
    #[must_use]
    pub fn with_rate_limit(mut self, limiter: RateLimiter) -> Self {
        self.limiter = Arc::new(limiter);
        self
    }

    /// Which service and network this client speaks to.
    #[must_use]
    pub const fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Whether this client carries a key, without saying what it is.
    #[must_use]
    pub const fn is_keyed(&self) -> bool {
        self.key.is_some()
    }

    /// Asks `path` with `query` and reads the answer as `T`.
    ///
    /// The call is paced before it goes out, and tried again when it is
    /// refused, never answered, or answered by a server error. A client error
    /// is reported rather than retried: a malformed request answered `400`
    /// five times is five wrong requests, not one transient failure.
    ///
    /// # Errors
    /// See [`ChainError`].
    pub fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Query<'_>,
    ) -> Result<T, ChainError> {
        // Read into the value directly rather than through a string: a page of
        // an indexer's answer runs to megabytes, and holding it twice to parse
        // it once is a copy nobody asked for.
        self.get(path, query, |response, provider, url| {
            response
                .body_mut()
                .read_json::<T>()
                .map_err(|source| ChainError::Body {
                    provider,
                    url,
                    source: Box::new(source),
                })
        })
    }

    /// Asks `path` with `query` and returns the answer's body as it arrived.
    ///
    /// For the caller that has to keep an answer verbatim: a sweep recording
    /// what an indexer said stores the bytes, because re-serialising a value
    /// reorders its keys and a record that cannot be compared byte for byte is
    /// not a record of what was said.
    ///
    /// # Errors
    /// See [`ChainError`].
    pub fn get_text(&self, path: &str, query: Query<'_>) -> Result<String, ChainError> {
        self.get(path, query, |response, provider, url| {
            response
                .body_mut()
                .read_to_string()
                .map_err(|source| ChainError::Body {
                    provider,
                    url,
                    source: Box::new(source),
                })
        })
    }

    /// The one request path: paced, keyed, retried, and read by `body`.
    ///
    /// The reading is the caller's because that is the only difference between
    /// the two public calls, and a second copy of the pacing and the retrying
    /// to serve that difference is how this crate's own rule gets broken inside
    /// this crate.
    fn get<T>(
        &self,
        path: &str,
        query: Query<'_>,
        read: impl Fn(
            &mut ureq::http::Response<ureq::Body>,
            &'static str,
            String,
        ) -> Result<T, ChainError>,
    ) -> Result<T, ChainError> {
        let url = self.endpoint.url(path);
        let provider = self.endpoint.provider.name();
        let hint = ApiKeys::variable_for(self.endpoint.provider, self.endpoint.network)
            .filter(|_| self.key.is_none());

        with_retries(&self.retry, provider, hint, |_attempt| {
            self.limiter.acquire();
            let mut request = self.agent.get(&url);
            for (name, value) in query {
                request = request.query(*name, *value);
            }
            if let Some(key) = &self.key {
                request = request.header(
                    self.endpoint.provider.key_header(),
                    self.endpoint.provider.key_header_value(key.expose()),
                );
            }
            let mut response = match request.call() {
                Ok(response) => response,
                // Transient by nature: a dropped connection answers differently
                // on the next attempt often enough that the library resolver
                // had its own retry loop for exactly this, and under-counted
                // every pass it did not.
                Err(source) => {
                    return Ok(Attempt::Unreachable {
                        cause: ChainError::Transport {
                            provider,
                            url: url.clone(),
                            source: Box::new(source),
                        },
                    });
                }
            };

            let status = response.status().as_u16();
            if status == RATE_LIMITED {
                return Ok(Attempt::RateLimited {
                    retry_after: retry_after::parse(response.headers()),
                });
            }
            if !response.status().is_success() {
                let unexpected = ChainError::UnexpectedStatus {
                    provider,
                    url: url.clone(),
                    status,
                };
                // A server error is the service saying it could not answer this
                // time, and the library resolver measured what happens when
                // that is taken as final: a pass that under-counts and picks up
                // more each time it is run again. A client error is the request
                // being wrong, and it will be as wrong next time.
                return if status >= SERVER_ERROR {
                    Ok(Attempt::Unreachable { cause: unexpected })
                } else {
                    Err(unexpected)
                };
            }
            read(&mut response, provider, url.clone()).map(Attempt::Done)
        })
    }
}

impl std::fmt::Debug for ChainClient {
    /// The endpoint, the pace, and whether there is a key — never the key.
    /// Written by hand because the transport agent has no `Debug` and because
    /// a derived one would put a secret one field away from every log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainClient")
            .field("endpoint", &self.endpoint)
            .field("keyed", &self.is_keyed())
            .field("interval", &self.limiter.interval())
            .field("retry", &self.retry)
            // The key and the transport agent are deliberately absent: one is
            // a secret, the other has nothing readable to print.
            .finish_non_exhaustive()
    }
}

/// The agent every client shares the shape of.
///
/// Statuses are read here rather than turned into errors by the transport,
/// and every attempt carries an end-to-end ceiling: without one a stalled
/// connection hangs a run, and backoff only ever engages after an attempt has
/// finished.
fn agent() -> Agent {
    let config: Config = Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(DEFAULT_CALL_TIMEOUT))
        .build();
    config.into()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::ChainClient;
    use crate::credentials::ApiKeys;
    use crate::endpoint::{Endpoint, Network, Provider};

    #[test]
    fn every_pair_of_provider_and_network_builds_a_client_at_its_own_host() {
        for provider in [
            Provider::ToncenterV3,
            Provider::ToncenterV2,
            Provider::TonapiV2,
        ] {
            for network in [Network::Mainnet, Network::Testnet] {
                let client = ChainClient::public(provider, network);
                assert_eq!(client.endpoint().provider, provider);
                assert_eq!(client.endpoint().network, network);
            }
        }
    }

    #[test]
    fn a_keyless_client_paces_itself_at_the_tier_it_actually_has() {
        // A pair no other test builds a real client for: the limiter registry
        // lives for the process, so two tests naming one service would share
        // one limiter and one of them would be asserting the other's pace.
        let endpoint = Endpoint::public(Provider::TonapiV2, Network::Mainnet);
        let client = ChainClient::keyed(endpoint, &ApiKeys::default());
        assert!(!client.is_keyed());
        assert_eq!(
            client.limiter.interval(),
            std::time::Duration::from_secs(1),
            "the public tier answers about one request a second"
        );
    }

    #[test]
    fn a_keyed_client_paces_itself_at_ten_a_second() {
        let endpoint = Endpoint::public(Provider::ToncenterV3, Network::Testnet);
        let keys = ApiKeys::from_reader(|name| {
            (name == "TONCENTER_API_KEY_TESTNET").then(|| "key".to_string())
        });
        let client = ChainClient::keyed(endpoint, &keys);
        assert!(client.is_keyed());
        assert_eq!(
            client.limiter.interval(),
            std::time::Duration::from_millis(100)
        );
    }
}
