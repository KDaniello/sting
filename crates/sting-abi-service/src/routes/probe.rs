//! `POST /v1/probe` (`docs/domains/service.md`) — Tier 3
//! (`sting-abi-dictionary`) bytecode-only method-id
//! probing, reachable over HTTP for the first time: until this route
//! existed, the only way to run a Tier 3 probe was the offline
//! `abi-layer dictionary-probe` CLI (`crates/sting-abi-cli/src/dictionary.rs`,
//! whose `run_probe` this handler's core logic mirrors directly — same
//! `ProbeTarget`/`unixtime`/`balance` construction, same
//! `sting-corpus`-backed on-chain fetch). No source is read or required
//! anywhere in this call — pure address-in, `ProbeReport`-out.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::response::IntoResponse;
use serde::Deserialize;

use sting_abi_dictionary::{ProbeReport, ProbeTarget, probe};
use sting_corpus::{ContractTarget, FetchOptions};

use crate::error::{ApiError, AppJson};
use crate::state::AppState;

/// `POST /v1/probe`'s request body.
#[derive(Debug, Deserialize)]
pub struct ProbeRequest {
    address: String,
}

pub async fn probe_address(
    State(state): State<AppState>,
    AppJson(request): AppJson<ProbeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let Some(corpus) = state.corpus.clone() else {
        return Err(ApiError::Tier3Unavailable);
    };
    let Some(dictionary) = state.dictionary.clone() else {
        return Err(ApiError::Tier3Unavailable);
    };
    let Some(emulator_lib) = state.emulator_lib.clone() else {
        return Err(ApiError::Tier3Unavailable);
    };
    let request_timeout = state.config.request_timeout;

    let join = tokio::task::spawn_blocking(move || -> Result<ProbeReport, ApiError> {
        let code = corpus
            .resolve(
                &ContractTarget::OnChainAddress(request.address.clone()),
                FetchOptions::default(),
            )
            .map_err(|source| {
                ApiError::OnChainResolutionFailed(format!("{:?}: {source}", request.address))
            })?;

        // Real unix timestamp for `c7`'s `NOW` — a plausible current time,
        // not a precisely-reproducible one, same reasoning as the CLI's
        // own `run_probe` (`crates/sting-abi-cli/src/dictionary.rs`).
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a real unix timestamp comfortably fits u32 until year 2106; this call site only needs a plausible current time for c7, not a precise one"
        )]
        let unixtime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as u32);

        let target = ProbeTarget {
            code_boc_base64: &code.code_boc_base64,
            data_boc_base64: &code.data_boc_base64,
            address: &request.address,
            unixtime,
            // Real balance isn't fetched by `CorpusController::resolve`
            // (code+data only) — same documented best-effort placeholder as
            // the CLI path; can only affect a *recognized* method's
            // returned values, never the presence/absence classification.
            balance: 0,
        };
        let report = probe(&emulator_lib, &target, &dictionary)?;
        Ok(report)
    });

    match tokio::time::timeout(request_timeout, join).await {
        Ok(Ok(result)) => result.map(AppJson),
        Ok(Err(join_error)) => Err(ApiError::Internal(format!(
            "probe task panicked: {join_error}"
        ))),
        Err(_elapsed) => Err(ApiError::Timeout),
    }
}
