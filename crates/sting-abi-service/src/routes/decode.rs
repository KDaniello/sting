//! `POST /v1/decode` — a hash in, an operation out, in words and fields.
//!
//! The one surface this platform's decode product is: a caller who has a transaction hash, or the
//! body itself, gets back what operation it is and what it carries. No source is read anywhere in
//! this call, and nothing is compiled — the answer comes from the learned datasets and the bits.
//!
//! **Not knowing is an answer, not an error.** An opcode no dataset names comes back as `200` with
//! no operations and `unknown: true`. A `404` would say the request was wrong, and it was not: the
//! caller asked a well-formed question about a real message, and the honest answer is that this
//! platform does not know that one. The share of requests answered that way is the number the
//! product is measured by, and a shape that hid it in an error code would hide the measurement too.

use axum::extract::State;
use axum::response::IntoResponse;
use base64::Engine as _;
use serde::Deserialize;
use sting_abi_coverage::MessageReader;
use sting_abi_decode::{Decoded, decode};

use crate::error::{ApiError, AppJson};
use crate::state::AppState;

/// `POST /v1/decode`'s request body: one of the two ways to name a message.
#[derive(Debug, Deserialize)]
pub struct DecodeRequest {
    /// A mainnet transaction hash, whose incoming message is read.
    #[serde(default)]
    hash: Option<String>,
    /// A message body, as base64 of its bag of cells — for a caller that already has the bytes and
    /// wants no chain request made on their behalf.
    #[serde(default)]
    body: Option<String>,
}

/// What this route answers with.
#[derive(Debug, serde::Serialize)]
pub struct DecodeResponse {
    /// Which version of this answer's shape it is in.
    pub schema_version: String,
    /// Whether nothing named the opcode. A plain field rather than something to infer from an empty
    /// list: the caller measuring this platform should not have to guess what empty means.
    pub unknown: bool,
    /// What the body turned out to be.
    #[serde(flatten)]
    pub decoded: Decoded,
}

/// This answer's own format version.
pub const DECODE_SCHEMA_VERSION: &str = "1.0";

pub async fn decode_message(
    State(state): State<AppState>,
    AppJson(request): AppJson<DecodeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let Some(operations) = state.operations.clone() else {
        return Err(ApiError::DecodeUnavailable);
    };
    let max_result_bytes = state.config.max_result_bytes;

    let body = match (request.body, request.hash) {
        (Some(text), _) => base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .map_err(|source| ApiError::InvalidRequest(format!("body is not base64: {source}")))?,
        (None, Some(hash)) => {
            let messages = state.messages.clone();
            // The chain call is blocking, and this crate's rule is that no blocking call runs on an
            // async handler's thread — the same rule `POST /v1/probe` follows.
            tokio::task::spawn_blocking(move || read_body_of(&messages, &hash))
                .await
                .map_err(|source| ApiError::Internal(format!("decode task failed: {source}")))??
        }
        (None, None) => {
            return Err(ApiError::InvalidRequest(
                "name a message: pass either hash or body".to_string(),
            ));
        }
    };

    let decoded = decode(&body, &operations)
        .map_err(|source| ApiError::InvalidRequest(format!("{source}")))?;
    let answer = DecodeResponse {
        schema_version: DECODE_SCHEMA_VERSION.to_string(),
        unknown: decoded.is_unknown(),
        decoded,
    };

    // The third limit, checked on what is actually about to be sent rather than on a proxy for it:
    // a body's reading is as long as its message's fields, and a caller who asked for a decode did
    // not ask to be handed a megabyte.
    let text = serde_json::to_string(&answer)
        .map_err(|source| ApiError::Internal(format!("serialising the answer: {source}")))?;
    if text.len() > max_result_bytes {
        return Err(ApiError::ResultTooLarge);
    }
    Ok(AppJson(answer))
}

/// Reads one transaction's incoming body off mainnet, through the deployment's one reader.
fn read_body_of(messages: &MessageReader, hash: &str) -> Result<Vec<u8>, ApiError> {
    let observed = messages
        .by_transaction(hash)
        .map_err(|source| ApiError::OnChainResolutionFailed(format!("{hash}: {source}")))?
        .ok_or_else(|| ApiError::InvalidRequest(format!("no transaction {hash} on mainnet")))?;
    observed.body.ok_or_else(|| {
        ApiError::InvalidRequest(format!("transaction {hash} carries no incoming body"))
    })
}
