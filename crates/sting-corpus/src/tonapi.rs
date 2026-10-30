//! `tonapi.io/v2` as a second source of contract code.
//!
//! The fallback, and what makes it useful is that its rate limit is its own:
//! a run that has spent the first provider's quota has not spent this one's.
//! The transport is `sting-chain`; what stays here is the path that answers
//! with an account and the shape of that answer.
//!
//! The response shape, confirmed by a live request: `GET
//! /blockchain/accounts/<addr>` returns `{"address": "...", "balance": ...,
//! "code": "<hex>", "data": "<hex>", "status": "active", ...}`. **`code` and
//! `data` are hex, not base64** — confirmed by comparing byte lengths against
//! the same address's toncenter answer, which decodes to the same bytes. That
//! is a real difference between the two services rather than an assumption
//! about one, and this module normalises it away: [`ContractCode`] always
//! carries base64, which is what the emulator reads.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use sting_chain::{ChainClient, Endpoint, Network, Provider, RetryPolicy};

use crate::error::FetchError;
use crate::source::{ContractCode, ContractSource};

const PROVIDER: &str = "tonapi v2";

/// Contract code from `tonapi.io/v2`.
pub struct TonapiClient {
    client: ChainClient,
}

impl TonapiClient {
    /// A client against mainnet.
    #[must_use]
    pub fn new() -> Self {
        Self::for_network(Network::Mainnet)
    }

    /// A client against `network`, keyed from the environment.
    #[must_use]
    pub fn for_network(network: Network) -> Self {
        Self::over(Endpoint::public(Provider::TonapiV2, network))
    }

    /// A client against another host — a stub server, or a self-hosted
    /// instance.
    #[must_use]
    pub fn with_base_url(base_url: String) -> Self {
        Self::over(Endpoint::at(Provider::TonapiV2, Network::Mainnet, base_url))
    }

    /// A client over `endpoint`, paced by whichever key the environment
    /// carries for it.
    #[must_use]
    pub fn over(endpoint: Endpoint) -> Self {
        Self {
            client: ChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env()),
        }
    }

    /// Overrides the retry policy.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.client = self.client.with_retry_policy(policy);
        self
    }
}

impl Default for TonapiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, serde::Deserialize)]
struct AccountResponse {
    code: Option<String>,
    data: Option<String>,
}

/// Percent-encodes `address` for use as one URL path segment.
///
/// `sting_address::validate_text` already restricts an address to
/// `[A-Za-z0-9:+/=_-]`, and every character in that set except `/` is a valid
/// RFC 3986 `pchar` needing no encoding. `/` is the separator itself and would
/// otherwise turn one segment into two. Escaping the one character this
/// already-narrow alphabet allows is what a percent-encoding dependency would
/// do here, in one line.
fn path_segment_encode(address: &str) -> String {
    address.replace('/', "%2F")
}

fn hex_field_to_base64(field: &'static str, hex_text: &str) -> Result<String, FetchError> {
    let bytes = hex::decode(hex_text).map_err(|source| FetchError::Hex {
        provider: PROVIDER,
        field,
        source,
    })?;
    Ok(BASE64.encode(bytes))
}

impl ContractSource for TonapiClient {
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
        let path = format!("/blockchain/accounts/{}", path_segment_encode(address));
        let body: AccountResponse = self.client.get_json(&path, &[])?;
        let code_hex = body.code.ok_or(FetchError::MissingField {
            provider: PROVIDER,
            field: "code",
        })?;
        let data_hex = body.data.ok_or(FetchError::MissingField {
            provider: PROVIDER,
            field: "data",
        })?;
        Ok(ContractCode {
            code_boc_base64: hex_field_to_base64("code", &code_hex)?,
            data_boc_base64: hex_field_to_base64("data", &data_hex)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ContractSource, TonapiClient, path_segment_encode};
    use crate::FetchError;

    #[test]
    fn fetch_rejects_a_malformed_address_before_any_network_call() {
        let client = TonapiClient::with_base_url("http://127.0.0.1:1".to_string());
        let result = client.fetch("0:not a valid address?");
        assert!(matches!(result, Err(FetchError::InvalidAddress { .. })));
    }

    #[test]
    fn path_segment_encode_escapes_the_path_separator() {
        assert_eq!(path_segment_encode("EQD/abc"), "EQD%2Fabc");
    }

    #[test]
    fn path_segment_encode_leaves_other_valid_address_characters_alone() {
        let address = "EQD+abc123=_-:xyz";
        assert_eq!(path_segment_encode(address), address);
    }
}
