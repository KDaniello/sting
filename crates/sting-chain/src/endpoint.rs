//! Which service, on which network, at which address.

use std::fmt;

/// Which chain a request is asked of.
///
/// Named rather than inferred from a base URL: a key issued for one network
/// answers nothing on the other, and a test that quietly ran against mainnet
/// is a test that spent somebody's quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Network {
    /// The live chain.
    Mainnet,
    /// The test chain.
    Testnet,
}

impl Network {
    /// The word this network is called by in an environment variable.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Testnet => "testnet",
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which public service answers.
///
/// Two, because neither answers everything this tree asks: the first will not
/// page a large collection past about ninety thousand items and the second
/// lists no transfers. Each is asked what it can answer, and which one
/// answered travels with the answer. Both publish a testnet host, confirmed
/// by a real request to each on 2026-08-22 rather than read off a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// `toncenter.com/api/v3`.
    ToncenterV3,
    /// `toncenter.com/api/v2` — the older API of the same service, kept for
    /// the one capability v3 does not publish: `getLibraries`, which resolves
    /// a library reference cell to the code it points at.
    ToncenterV2,
    /// `tonapi.io/v2`.
    TonapiV2,
}

impl Provider {
    /// The name this provider is reported by in an error or a diagnostic.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ToncenterV3 => "toncenter v3",
            Self::ToncenterV2 => "toncenter v2",
            Self::TonapiV2 => "tonapi v2",
        }
    }

    /// The header this provider expects its key in.
    ///
    /// Different by provider, and confirmed against each service rather than
    /// against its documentation: on 2026-08-22 a deliberately wrong key was
    /// sent to both in both headers. Toncenter answered `401` to `X-Api-Key`
    /// and ignored the bearer; tonapi answered `401` to the bearer and ignored
    /// `X-Api-Key`. A key in the header a service ignores is a key that buys
    /// nothing and says so nowhere.
    #[must_use]
    pub const fn key_header(self) -> &'static str {
        match self {
            Self::ToncenterV3 | Self::ToncenterV2 => "X-Api-Key",
            Self::TonapiV2 => "Authorization",
        }
    }

    /// How this provider's key is written into that header.
    #[must_use]
    pub fn key_header_value(self, key: &str) -> String {
        match self {
            Self::ToncenterV3 | Self::ToncenterV2 => key.to_string(),
            Self::TonapiV2 => format!("Bearer {key}"),
        }
    }

    /// Where this provider answers for `network`.
    ///
    /// Every pair has a host, and each of the six answered a live request on
    /// 2026-08-22. It is a value rather than a `Maybe` because the alternative
    /// was measured in this tree: six callers each wrote their own fallback to
    /// an empty host, which turns a service that stopped publishing into a
    /// request to nowhere. A provider that really stops publishing one is a
    /// change to this table and to whatever depends on it, made visible by the
    /// compiler.
    #[must_use]
    pub const fn default_base_url(self, network: Network) -> &'static str {
        match (self, network) {
            (Self::ToncenterV3, Network::Mainnet) => "https://toncenter.com/api/v3",
            (Self::ToncenterV3, Network::Testnet) => "https://testnet.toncenter.com/api/v3",
            (Self::ToncenterV2, Network::Mainnet) => "https://toncenter.com/api/v2",
            (Self::ToncenterV2, Network::Testnet) => "https://testnet.toncenter.com/api/v2",
            (Self::TonapiV2, Network::Mainnet) => "https://tonapi.io/v2",
            (Self::TonapiV2, Network::Testnet) => "https://testnet.tonapi.io/v2",
        }
    }
}

/// One service on one network at one base URL.
///
/// The base URL is part of the value rather than a constant, because a stub
/// server in a container is the only way to test a retry and a rate limit
/// without asking a public service to misbehave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Which service.
    pub provider: Provider,
    /// Which chain.
    pub network: Network,
    /// Base URL with no trailing slash.
    pub base_url: String,
}

impl Endpoint {
    /// The provider's own published host for `network`.
    #[must_use]
    pub fn public(provider: Provider, network: Network) -> Self {
        Self {
            provider,
            network,
            base_url: provider.default_base_url(network).to_string(),
        }
    }

    /// The same provider and network at another host — a stub server, or a
    /// self-hosted instance.
    #[must_use]
    pub fn at(provider: Provider, network: Network, base_url: impl Into<String>) -> Self {
        Self {
            provider,
            network,
            base_url: base_url.into(),
        }
    }

    /// The full URL for `path`, which is expected to start with `/`.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{Endpoint, Network, Provider};

    #[test]
    fn every_pair_of_provider_and_network_names_a_host() {
        for provider in [
            Provider::ToncenterV3,
            Provider::ToncenterV2,
            Provider::TonapiV2,
        ] {
            for network in [Network::Mainnet, Network::Testnet] {
                let endpoint = Endpoint::public(provider, network);
                assert!(
                    endpoint.base_url.starts_with("https://"),
                    "{provider:?} on {network}: {}",
                    endpoint.base_url
                );
            }
        }
    }

    #[test]
    fn the_two_networks_of_one_provider_are_different_hosts() {
        assert_ne!(
            Endpoint::public(Provider::ToncenterV3, Network::Mainnet).base_url,
            Endpoint::public(Provider::ToncenterV3, Network::Testnet).base_url
        );
    }

    #[test]
    fn a_trailing_slash_in_the_base_url_does_not_double_in_the_path() {
        let endpoint = Endpoint::at(
            Provider::ToncenterV3,
            Network::Mainnet,
            "http://localhost/api/",
        );
        assert_eq!(endpoint.url("/account"), "http://localhost/api/account");
    }

    #[test]
    fn each_provider_carries_its_own_key_header() {
        assert_eq!(Provider::ToncenterV3.key_header(), "X-Api-Key");
        assert_eq!(Provider::ToncenterV3.key_header_value("k"), "k");
        assert_eq!(Provider::TonapiV2.key_header(), "Authorization");
        assert_eq!(Provider::TonapiV2.key_header_value("k"), "Bearer k");
    }
}
