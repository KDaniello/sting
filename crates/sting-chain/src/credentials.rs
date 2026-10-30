//! The keys, read from the environment and printed nowhere.
//!
//! A key is a secret with a habit of ending up in a log line, a snapshot or a
//! panic message, and each of those is a leak that looks like a diagnostic.
//! [`ApiKey`] therefore has a `Debug` that prints its length and nothing else,
//! and no `Display` at all: the only way to the bytes is [`ApiKey::expose`],
//! whose name is what a reviewer greps for.

use std::fmt;

use crate::endpoint::{Network, Provider};

/// One key, carried without being printable.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wraps a key read from somewhere the caller trusts.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key itself, for the one place that puts it in a request header.
    ///
    /// Named for what it does rather than for what it returns: a call to
    /// `expose` in a log statement is visible in review, a call to `as_str`
    /// is not.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    /// Length only. A key that renders itself under `{:?}` reaches every log
    /// line that formats a struct containing it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiKey(<{} chars, hidden>)", self.0.len())
    }
}

/// Every key this tree knows how to use, by provider and network.
///
/// A missing key is not an error: without one the public tier answers about
/// one request a second instead of ten, which is slower and not different.
/// What is refused is a key that exists and is wrong — that comes back from
/// the provider as a status, and a status is not this type's business.
#[derive(Debug, Clone, Default)]
pub struct ApiKeys {
    toncenter_mainnet: Option<ApiKey>,
    toncenter_testnet: Option<ApiKey>,
    tonapi_mainnet: Option<ApiKey>,
    tonapi_testnet: Option<ApiKey>,
}

/// The variable each key is read from. One per network, because a key issued
/// for one network answers nothing on the other.
const TONCENTER_MAINNET: &str = "TONCENTER_API_KEY_MAINNET";
const TONCENTER_TESTNET: &str = "TONCENTER_API_KEY_TESTNET";
const TONAPI_MAINNET: &str = "TONAPI_API_KEY_MAINNET";
const TONAPI_TESTNET: &str = "TONAPI_API_KEY_TESTNET";

impl ApiKeys {
    /// Reads every key from the environment, after loading `.env` if one is
    /// there.
    ///
    /// Read once for the life of the process and handed out cloned. Two risk
    /// signals in this tree build a client per call, and a client per call was
    /// a `.env` opened per call — a file read on the path of every request to
    /// answer a question whose answer cannot change under a running process.
    ///
    /// A `.env` that is absent is the ordinary case rather than a failure: the
    /// public tier works without any key, and a fresh clone has none by
    /// construction — the file is gitignored, and committing one is how a key
    /// reaches a repository.
    #[must_use]
    pub fn from_env() -> Self {
        static KEYS: std::sync::OnceLock<ApiKeys> = std::sync::OnceLock::new();
        KEYS.get_or_init(|| {
            let _ = dotenvy::dotenv();
            Self::from_reader(|name| std::env::var(name).ok())
        })
        .clone()
    }

    /// The same, reading through `read` instead of the process environment —
    /// what a test uses, so that no test has to mutate a global.
    #[must_use]
    pub fn from_reader(mut read: impl FnMut(&str) -> Option<String>) -> Self {
        let key = |read: &mut dyn FnMut(&str) -> Option<String>, name: &str| {
            read(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(ApiKey::new)
        };
        Self {
            toncenter_mainnet: key(&mut read, TONCENTER_MAINNET),
            toncenter_testnet: key(&mut read, TONCENTER_TESTNET),
            tonapi_mainnet: key(&mut read, TONAPI_MAINNET),
            tonapi_testnet: key(&mut read, TONAPI_TESTNET),
        }
    }

    /// The key for one provider on one network, if there is one.
    #[must_use]
    pub fn get(&self, provider: Provider, network: Network) -> Option<&ApiKey> {
        match (provider, network) {
            // One key per service and network, and v2 and v3 are one service:
            // the key that answers on one answers on the other.
            (Provider::ToncenterV3 | Provider::ToncenterV2, Network::Mainnet) => {
                self.toncenter_mainnet.as_ref()
            }
            (Provider::ToncenterV3 | Provider::ToncenterV2, Network::Testnet) => {
                self.toncenter_testnet.as_ref()
            }
            (Provider::TonapiV2, Network::Mainnet) => self.tonapi_mainnet.as_ref(),
            (Provider::TonapiV2, Network::Testnet) => self.tonapi_testnet.as_ref(),
        }
    }

    /// Which variable a caller should set to raise a given pair off the
    /// public tier — the message a slow run is worth printing.
    #[must_use]
    pub const fn variable_for(provider: Provider, network: Network) -> Option<&'static str> {
        match (provider, network) {
            (Provider::ToncenterV3 | Provider::ToncenterV2, Network::Mainnet) => {
                Some(TONCENTER_MAINNET)
            }
            (Provider::ToncenterV3 | Provider::ToncenterV2, Network::Testnet) => {
                Some(TONCENTER_TESTNET)
            }
            (Provider::TonapiV2, Network::Mainnet) => Some(TONAPI_MAINNET),
            (Provider::TonapiV2, Network::Testnet) => Some(TONAPI_TESTNET),
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
    use super::{ApiKey, ApiKeys, TONCENTER_MAINNET, TONCENTER_TESTNET};
    use crate::endpoint::{Network, Provider};

    #[test]
    fn a_key_never_renders_itself_under_debug() {
        let key = ApiKey::new("aaaabbbbccccdddd");
        let rendered = format!("{key:?}");
        assert!(!rendered.contains("aaaabbbb"), "{rendered}");
        assert!(rendered.contains("16 chars"), "{rendered}");
    }

    #[test]
    fn a_struct_holding_a_key_does_not_leak_it_through_its_own_debug() {
        let keys = ApiKeys::from_reader(|name| {
            (name == TONCENTER_MAINNET).then(|| "secret-value".to_string())
        });
        let rendered = format!("{keys:?}");
        assert!(!rendered.contains("secret-value"), "{rendered}");
    }

    #[test]
    fn each_network_reads_its_own_variable() {
        let keys = ApiKeys::from_reader(|name| match name {
            TONCENTER_MAINNET => Some("live".to_string()),
            TONCENTER_TESTNET => Some("test".to_string()),
            _ => None,
        });
        assert_eq!(
            keys.get(Provider::ToncenterV3, Network::Mainnet)
                .map(ApiKey::expose),
            Some("live")
        );
        assert_eq!(
            keys.get(Provider::ToncenterV3, Network::Testnet)
                .map(ApiKey::expose),
            Some("test")
        );
    }

    #[test]
    fn an_empty_variable_is_no_key_rather_than_an_empty_one() {
        // A key set to the empty string would otherwise be sent as a header
        // and answered with a status nobody can read back to a typo.
        let keys =
            ApiKeys::from_reader(|name| (name == TONCENTER_MAINNET).then(|| "   ".to_string()));
        assert!(keys.get(Provider::ToncenterV3, Network::Mainnet).is_none());
    }

    #[test]
    fn every_pair_that_has_a_host_names_the_variable_its_key_is_read_from() {
        for provider in [
            Provider::ToncenterV3,
            Provider::ToncenterV2,
            Provider::TonapiV2,
        ] {
            for network in [Network::Mainnet, Network::Testnet] {
                assert!(
                    ApiKeys::variable_for(provider, network).is_some(),
                    "{provider:?} on {network}"
                );
            }
        }
    }
}
