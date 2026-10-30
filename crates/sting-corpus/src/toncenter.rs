//! `toncenter.com/api/v3` as a source of contract code.
//!
//! The transport is not here. Pacing, retrying, honouring a `Retry-After` and
//! carrying a key belong to `sting-chain`, and what is left in this module is
//! the one thing that is specific to this provider: which path answers with an
//! account, and what its answer's fields mean.
//!
//! The response shape, confirmed by a live request rather than assumed from
//! documentation: `GET /account?address=<addr>` returns `{"balance": "...",
//! "code": "<base64 BoC>", "data": "<base64 BoC>", "status": "active", ...}`.
//! `code` and `data` are already base64 `BoC`s, which is exactly what
//! [`ContractCode`] stores — no further decoding, unlike [`crate::tonapi`],
//! which answers in hex.

use sting_chain::{ChainClient, Endpoint, Network, Provider, RetryPolicy};

use crate::error::FetchError;
use crate::source::{ContractCode, ContractSource};

const PROVIDER: &str = "toncenter v3";

/// Contract code from `toncenter.com/api/v3`.
pub struct ToncenterClient {
    client: ChainClient,
}

impl ToncenterClient {
    /// A client against mainnet.
    #[must_use]
    pub fn new() -> Self {
        Self::for_network(Network::Mainnet)
    }

    /// A client against `network`, keyed from the environment.
    #[must_use]
    pub fn for_network(network: Network) -> Self {
        Self::over(Endpoint::public(Provider::ToncenterV3, network))
    }

    /// A client against another host — a stub server, or a self-hosted
    /// instance. The network is taken to be mainnet, which is what a
    /// self-hosted instance of this API serves unless it says otherwise.
    #[must_use]
    pub fn with_base_url(base_url: String) -> Self {
        Self::over(Endpoint::at(
            Provider::ToncenterV3,
            Network::Mainnet,
            base_url,
        ))
    }

    /// A client over `endpoint`, paced by whichever key the environment
    /// carries for it.
    #[must_use]
    pub fn over(endpoint: Endpoint) -> Self {
        Self {
            client: ChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env()),
        }
    }

    /// Overrides the retry policy — what a test against a stub server needs so
    /// that exhausting the retries takes milliseconds rather than seconds.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.client = self.client.with_retry_policy(policy);
        self
    }
}

impl Default for ToncenterClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, serde::Deserialize)]
struct AccountResponse {
    code: Option<String>,
    data: Option<String>,
}

impl ContractSource for ToncenterClient {
    fn name(&self) -> &'static str {
        PROVIDER
    }

    fn fetch(&self, address: &str) -> Result<ContractCode, FetchError> {
        // Refused here rather than by the service: an address carrying a
        // character that would change the request never becomes part of one.
        sting_address::validate_text(address).map_err(|error| FetchError::InvalidAddress {
            provider: PROVIDER,
            address: error.address,
        })?;
        let body: AccountResponse = self.client.get_json("/account", &[("address", address)])?;
        let code = body.code.ok_or(FetchError::MissingField {
            provider: PROVIDER,
            field: "code",
        })?;
        let data = body.data.ok_or(FetchError::MissingField {
            provider: PROVIDER,
            field: "data",
        })?;
        Ok(ContractCode {
            code_boc_base64: code,
            data_boc_base64: data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ContractSource, ToncenterClient};
    use crate::FetchError;

    #[test]
    fn fetch_rejects_a_malformed_address_before_any_network_call() {
        // The base URL is never reachable: had validation not run first, this
        // would hang or come back as a transport failure rather than as a
        // refusal naming the address.
        let client = ToncenterClient::with_base_url("http://127.0.0.1:1".to_string());
        let result = client.fetch("0:not a valid address?");
        assert!(matches!(result, Err(FetchError::InvalidAddress { .. })));
    }
}
