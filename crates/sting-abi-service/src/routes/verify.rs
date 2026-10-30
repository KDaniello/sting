//! `POST /v1/verify` (`docs/domains/service.md`
//! §5.5) — the permissionless entry point anyone uses to verify their own
//! `(address, source)` pair, no API key/account, same principle as the
//! rest of this service.

use std::path::Path as StdPath;
use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use sting_abi_core::ContractSchema;
use sting_abi_registry::{MatchStatus, RegistryController};
use sting_corpus::ContractTarget;

use crate::error::{ApiError, AppJson};
use crate::extract_source::synthetic_entrypoint_name;
use crate::routes::extract::parse_lang;
use crate::state::AppState;

/// `POST /v1/verify`'s request body.
#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    address: String,
    source: String,
    lang: String,
}

/// `POST /v1/verify`'s response.
#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    status: MatchStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Box<ContractSchema>>,
}

pub async fn verify(
    State(state): State<AppState>,
    AppJson(request): AppJson<VerifyRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let lang = parse_lang(&request.lang)?;
    let Some(corpus) = state.corpus.clone() else {
        return Err(ApiError::RegistryUnavailable);
    };
    let bytecode_sources = Arc::clone(&state.bytecode_sources);
    let extract_controller = Arc::clone(&state.controller);
    let verified_store = Arc::clone(&state.verified_store);
    let request_timeout = state.config.request_timeout;
    let address = request.address.clone();

    let join = tokio::task::spawn_blocking(move || -> Result<VerifyResponse, ApiError> {
        let entrypoint_name = synthetic_entrypoint_name(lang);
        let scratch = tempfile::tempdir().map_err(|source| {
            ApiError::Internal(format!("failed to create a scratch directory: {source}"))
        })?;
        std::fs::write(
            scratch.path().join(entrypoint_name),
            request.source.as_bytes(),
        )
        .map_err(|source| {
            ApiError::Internal(format!("failed to write scratch source: {source}"))
        })?;

        let registry = RegistryController::new(&corpus, &bytecode_sources, &extract_controller);
        let target = ContractTarget::OnChainAddress(request.address);
        let outcome =
            registry.verify(&target, scratch.path(), StdPath::new(entrypoint_name), lang)?;

        // Best-effort: a corpus-growth persistence failure is our own
        // bookkeeping problem, not a reason to fail a response that
        // correctly answered the client's actual question ("does this
        // match"). Logged, not propagated
        // (`docs/domains/abi-ir.md`).
        if let (MatchStatus::Full | MatchStatus::Partial, Some(schema)) =
            (outcome.status, &outcome.schema)
            && let Err(err) = verified_store.put(&address, schema, outcome.status)
        {
            tracing::warn!(%address, %err, "failed to persist verification candidate");
        }

        Ok(VerifyResponse {
            status: outcome.status,
            schema: outcome.schema.map(Box::new),
        })
    });

    match tokio::time::timeout(request_timeout, join).await {
        Ok(Ok(result)) => result.map(AppJson),
        Ok(Err(join_error)) => Err(ApiError::Internal(format!(
            "verification task panicked: {join_error}"
        ))),
        Err(_elapsed) => Err(ApiError::Timeout),
    }
}
