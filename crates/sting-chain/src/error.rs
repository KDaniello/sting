//! What a request to the chain can fail as.
//!
//! One enum for every provider rather than one per provider: the shapes a
//! blocking JSON API fails in are the same whichever host answers, and which
//! host it was is a field. A caller matching on the shape stays correct when a
//! second provider is added; a caller matching on the provider does not.

/// A failed request.
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    /// The request never got an answer: connection refused, TLS, DNS, a
    /// malformed URL, or the per-call timeout.
    #[error("{provider}: the request to {url} did not complete: {source}")]
    Transport {
        /// Which service was asked.
        provider: &'static str,
        /// The URL that was requested.
        url: String,
        /// The underlying transport error.
        #[source]
        source: Box<ureq::Error>,
    },
    /// Every attempt was rate-limited.
    #[error(
        "{provider}: rate-limited after {attempts} attempt(s){}",
        .hint.map(|name| format!("; set {name} to raise the pace off the public tier")).unwrap_or_default()
    )]
    RateLimited {
        /// Which service refused.
        provider: &'static str,
        /// How many attempts were made.
        attempts: u32,
        /// The environment variable that would buy a faster tier, when there
        /// is one. A refusal that does not say what to do about it makes the
        /// caller go and read the source.
        hint: Option<&'static str>,
    },
    /// The service answered with a status this crate does not read: anything
    /// but a success or a rate limit.
    #[error("{provider}: unexpected status {status} from {url}")]
    UnexpectedStatus {
        /// Which service answered.
        provider: &'static str,
        /// The URL that was requested.
        url: String,
        /// The status that came back.
        status: u16,
    },
    /// The body was not the JSON it was asked for.
    #[error("{provider}: the answer from {url} is not readable JSON: {source}")]
    Body {
        /// Which service answered.
        provider: &'static str,
        /// The URL that was requested.
        url: String,
        /// The underlying parse error.
        #[source]
        source: Box<ureq::Error>,
    },
    /// The body arrived and is not the JSON the caller asked for.
    #[error("{provider}: the answer from {url} is not the JSON this call expects: {source}")]
    Unreadable {
        /// Which service answered.
        provider: &'static str,
        /// The URL that was requested.
        url: String,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// The call was dropped before it answered — the runtime carrying it shut
    /// down, or the task was cancelled. From the caller's side that is no
    /// answer, which is why it is an error rather than a silent `None`.
    #[error("the request was dropped before it answered: {reason}")]
    Cancelled {
        /// What the runtime said about it.
        reason: String,
    },
    /// The provider publishes no host for the network that was asked for.
    #[error("{provider} publishes no host for {network}")]
    NoSuchNetwork {
        /// Which service.
        provider: &'static str,
        /// Which network was asked for.
        network: &'static str,
    },
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::ChainError;

    #[test]
    fn a_rate_limit_names_the_variable_that_would_lift_it() {
        let error = ChainError::RateLimited {
            provider: "toncenter v3",
            attempts: 5,
            hint: Some("TONCENTER_API_KEY_MAINNET"),
        };
        let message = error.to_string();
        assert!(message.contains("TONCENTER_API_KEY_MAINNET"), "{message}");
    }

    #[test]
    fn a_rate_limit_with_no_variable_to_name_says_only_what_happened() {
        let error = ChainError::RateLimited {
            provider: "tonapi v2",
            attempts: 3,
            hint: None,
        };
        let message = error.to_string();
        assert!(message.ends_with("3 attempt(s)"), "{message}");
    }
}
