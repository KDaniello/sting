//! The asynchronous caller's way in: the same core, off the runtime's threads.
//!
//! One of the five callers this crate replaced is asynchronous, and this is
//! what it gets — not a second client. The core stays synchronous because four
//! of the five are, and a library that pulls a runtime into a linter's
//! dependency path to make one HTTP call has decided for its callers.
//!
//! The pace, the retries and the key are the core's, which is the whole point:
//! two clients would be two policies, and an adapter that reimplemented either
//! would be the sixth copy this crate exists to remove.
//!
//! # What this does not do
//!
//! It is not asynchronous underneath. A call occupies one blocking thread for
//! its duration, including the time it spends waiting out the pace, and
//! `tokio` is what keeps that off the runtime's own threads. That is a real
//! difference from a natively asynchronous client and it has a cost: a caller
//! issuing thousands of concurrent requests will hold thousands of blocking
//! threads, and the pace is what stops it — ten requests a second is ten
//! threads a second, not ten thousand.

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::client::ChainClient;
use crate::credentials::ApiKeys;
use crate::endpoint::{Endpoint, Network, Provider};
use crate::error::ChainError;

/// A [`ChainClient`] an asynchronous caller can await.
///
/// Cheap to clone: every clone shares one client, and therefore one pace. Two
/// clients would be two paces against one service, which is what the public
/// tier refuses.
#[derive(Clone, Debug)]
pub struct AsyncChainClient {
    inner: Arc<ChainClient>,
}

impl AsyncChainClient {
    /// The public host for `provider` on `network`, keyed from the
    /// environment.
    #[must_use]
    pub fn public(provider: Provider, network: Network) -> Self {
        Self::over(ChainClient::public(provider, network))
    }

    /// A client for `endpoint`, taking its key from `keys`.
    #[must_use]
    pub fn keyed(endpoint: Endpoint, keys: &ApiKeys) -> Self {
        Self::over(ChainClient::keyed(endpoint, keys))
    }

    /// The adapter over an already-built client — what a caller uses to set a
    /// retry policy or a pace of its own before wrapping it.
    #[must_use]
    pub fn over(client: ChainClient) -> Self {
        Self {
            inner: Arc::new(client),
        }
    }

    /// Which service and network this client speaks to.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        self.inner.endpoint()
    }

    /// Asks `path` with `query` and reads the answer as `T`.
    ///
    /// The query is owned rather than borrowed: the call outlives this
    /// function on another thread, and a borrow would tie it to a frame that
    /// has already returned.
    ///
    /// # Errors
    /// See [`ChainError`]. A runtime shutting down while the call is in flight
    /// is reported as a transport failure, because from the caller's side it
    /// is exactly that: no answer.
    pub async fn get_json<T: DeserializeOwned + Send + 'static>(
        &self,
        path: impl Into<String>,
        query: Vec<(String, String)>,
    ) -> Result<T, ChainError> {
        let client = Arc::clone(&self.inner);
        let path = path.into();
        let joined = tokio::task::spawn_blocking(move || {
            let borrowed: Vec<(&str, &str)> = query
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            client.get_json::<T>(&path, &borrowed)
        })
        .await;
        match joined {
            Ok(result) => result,
            Err(error) => Err(ChainError::Cancelled {
                reason: error.to_string(),
            }),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::AsyncChainClient;
    use crate::endpoint::{Network, Provider};

    #[test]
    fn a_clone_shares_one_client_and_therefore_one_pace() {
        let client = AsyncChainClient::public(Provider::ToncenterV3, Network::Mainnet);
        let second = client.clone();
        assert!(std::ptr::eq(
            std::ptr::from_ref(client.inner.as_ref()),
            std::ptr::from_ref(second.inner.as_ref())
        ));
    }
}
