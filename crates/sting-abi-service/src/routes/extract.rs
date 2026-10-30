//! `POST /v1/extract`, `POST /v1/extract/batch`
//! (`docs/domains/service.md`,
//! `docs/domains/abi-ir.md`).

use std::net::SocketAddr;
use std::path::Path as StdPath;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::response::IntoResponse;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use sting_abi_core::{ContractSchema, SourceLanguage};
use sting_abi_pipeline::{ExtractController, LanguageHint};

use crate::error::{ApiError, AppJson};
use crate::extract_source::synthetic_entrypoint_name;
use crate::state::AppState;

/// `POST /v1/extract`'s request body.
#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    source: String,
    lang: String,
}

/// `POST /v1/extract/batch`'s request body
/// (`docs/domains/abi-ir.md`).
#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    items: Vec<BatchItem>,
}

/// One element of a batch request — `id` is an opaque client-chosen
/// identifier (not necessarily a contract address: Tier 1/2 batch items
/// may have no on-chain address at all), echoed back unchanged so the
/// client can match inputs to outputs by `id` rather than by array order.
#[derive(Debug, Deserialize)]
pub struct BatchItem {
    id: String,
    source: String,
    lang: String,
}

/// One element of `POST /v1/extract/batch`'s response
/// (`docs/domains/abi-ir.md`) — a plain struct
/// with optional `schema`/`error`, not an internally-tagged enum: the spec
/// shows a flat `{id, status, schema|error}` object per item, which this
/// mirrors directly rather than introducing serde tagging machinery for a
/// shape simple enough not to need it.
#[derive(Debug, Serialize)]
pub struct BatchResultItem {
    id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Box<ContractSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// `POST /v1/extract/batch`'s response envelope.
#[derive(Debug, Serialize)]
pub struct BatchResponse {
    results: Vec<BatchResultItem>,
}

/// Parses the request body's `lang` field. `"auto"` is always rejected
/// here (unlike `sting-abi-cli`'s own `--lang`/auto-detect, which detects
/// from a real file extension): an inline HTTP `source` string has no
/// filename to detect from at all, and `docs/abi-layer/blocks/
/// 02-normalized-ir-cli.md` §2.2 already rejected content-sniffing as a
/// detection strategy on precision grounds — there's no honest way to
/// implement `"auto"` for this endpoint, so it's a clear client-facing
/// error instead of a silent wrong guess.
pub(crate) fn parse_lang(raw: &str) -> Result<SourceLanguage, ApiError> {
    match raw {
        "tolk" => Ok(SourceLanguage::Tolk),
        "tact" => Ok(SourceLanguage::Tact),
        "func" => Ok(SourceLanguage::Func),
        "auto" => Err(ApiError::InvalidRequest(
            "\"auto\" language detection requires a real filename to detect from, which an \
             inline HTTP `source` body does not have — pass an explicit \"lang\" \
             (\"tolk\"/\"tact\"/\"func\") instead"
                .to_string(),
        )),
        other => Err(ApiError::InvalidRequest(format!(
            "unknown \"lang\" value {other:?} — expected \"tolk\", \"tact\", or \"func\""
        ))),
    }
}

/// Writes `source` to a scratch directory under a synthetic entrypoint
/// name and runs it through `controller` — the one piece of blocking,
/// synchronous work (`docs/domains/abi-ir.md`:
/// `sting-abi-pipeline` never becomes async) every route in this module
/// wraps in `spawn_blocking` before calling.
fn extract_blocking(
    controller: &ExtractController,
    source: &str,
    lang: SourceLanguage,
) -> Result<ContractSchema, ApiError> {
    let entrypoint_name = synthetic_entrypoint_name(lang);
    let scratch = tempfile::tempdir().map_err(|source| {
        ApiError::Internal(format!("failed to create a scratch directory: {source}"))
    })?;
    let entrypoint_path = scratch.path().join(entrypoint_name);
    std::fs::write(&entrypoint_path, source.as_bytes()).map_err(|source| {
        ApiError::Internal(format!("failed to write scratch source: {source}"))
    })?;
    controller
        .extract(
            scratch.path(),
            StdPath::new(entrypoint_name),
            LanguageHint::Explicit(lang),
        )
        .map_err(ApiError::from)
}

pub async fn extract_single(
    State(state): State<AppState>,
    AppJson(request): AppJson<ExtractRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let lang = parse_lang(&request.lang)?;
    let controller = Arc::clone(&state.controller);
    let request_timeout = state.config.request_timeout;

    let join =
        tokio::task::spawn_blocking(move || extract_blocking(&controller, &request.source, lang));

    match tokio::time::timeout(request_timeout, join).await {
        Ok(Ok(result)) => result.map(AppJson),
        Ok(Err(join_error)) => Err(ApiError::Internal(format!(
            "extraction task panicked: {join_error}"
        ))),
        Err(_elapsed) => Err(ApiError::Timeout),
    }
}

pub async fn extract_batch(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    AppJson(request): AppJson<BatchRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if request.items.len() > state.config.batch_max_items {
        return Err(ApiError::InvalidRequest(format!(
            "batch has {} items, exceeding the configured limit of {}",
            request.items.len(),
            state.config.batch_max_items
        )));
    }

    // `rate_limit_middleware` already charged this request's flat 1 token
    // (same as every other extraction route) before this handler ever ran
    // — this charges the *marginal* cost of the remaining `N - 1` items, so
    // an N-item batch ends up costing N tokens in total, not 1. Rejected
    // here, before any compiler subprocess is spawned, not after — the
    // whole point is to avoid paying the compile cost for a batch that was
    // never going to be allowed. See `RateLimiter::check_n`'s doc comment
    // for the real amplification finding this closes
    // and the capacity-vs-batch_max_items
    // interaction it implies.
    // `items.len()` is bounded by `batch_max_items` (checked just above,
    // realistically at most a few hundred) — nowhere near 2^52, so `as f64`
    // losing precision above that threshold is not a reachable concern
    // here, just a lint that can't see the practical bound.
    #[allow(clippy::cast_precision_loss)]
    let marginal_cost = (request.items.len().saturating_sub(1)) as f64;
    if marginal_cost > 0.0 && !state.rate_limiter.check_n(addr.ip(), marginal_cost) {
        return Err(ApiError::RateLimited);
    }

    let controller = Arc::clone(&state.controller);
    let request_timeout = state.config.request_timeout;
    let items = request.items;

    // The whole batch — `rayon`-parallelized fan-out across items — runs
    // inside ONE `spawn_blocking` call, not one call per item:
    // `docs/domains/service.md` puts the `rayon` fan-out inside the handler
    // and wraps the handler in `spawn_blocking`, because `rayon`'s
    // `par_iter` blocks its calling thread until every item finishes, so
    // that calling thread must itself already be a blocking-pool thread,
    // not a `tokio` async worker.
    let join = tokio::task::spawn_blocking(move || {
        items
            .into_par_iter()
            .map(|item| extract_batch_item(&controller, &item))
            .collect::<Vec<_>>()
    });

    // One shared timeout for the whole batch, not per item: a batch that
    // cannot finish within budget fails as a whole with a clear `timeout`,
    // rather than silently returning partial results for whichever items
    // happened to finish first — the client already gets per-item
    // partial-failure via `status: "error"`; this timeout is about the
    // *request* as a whole.
    match tokio::time::timeout(request_timeout, join).await {
        Ok(Ok(results)) => Ok(AppJson(BatchResponse { results })),
        Ok(Err(join_error)) => Err(ApiError::Internal(format!(
            "batch extraction task panicked: {join_error}"
        ))),
        Err(_elapsed) => Err(ApiError::Timeout),
    }
}

fn extract_batch_item(controller: &ExtractController, item: &BatchItem) -> BatchResultItem {
    let id = item.id.clone();
    let lang = match parse_lang(&item.lang) {
        Ok(lang) => lang,
        Err(err) => {
            return BatchResultItem {
                id,
                status: "error",
                schema: None,
                error: Some(err.batch_message()),
            };
        }
    };

    // Isolates a panic to this one item. Without `catch_unwind`, `rayon`'s
    // `into_par_iter().collect()` in `extract_batch` propagates a single
    // item's panic and discards the whole batch's already-computed
    // results — a real bug found by review, reproduced empirically with a
    // panicking mock extractor mixed into an otherwise-successful batch —
    // violating the rule that one invalid item answers `status: error` and
    // does not take the rest of the batch down with it. No
    // extraction-path code is currently known to panic, but this isolates
    // the *whole batch* from any future one that does, matching the same
    // per-item blast radius `Result::Err` already gets below.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_blocking(controller, &item.source, lang)
    })) {
        Ok(Ok(schema)) => BatchResultItem {
            id,
            status: "ok",
            schema: Some(Box::new(schema)),
            error: None,
        },
        Ok(Err(err)) => BatchResultItem {
            id,
            status: "error",
            schema: None,
            error: Some(err.batch_message()),
        },
        Err(panic_payload) => {
            let message = panic_message(&panic_payload);
            tracing::error!(item_id = %id, panic = %message, "batch item extraction panicked");
            BatchResultItem {
                id,
                status: "error",
                schema: None,
                error: Some(format!("internal_error: extraction panicked: {message}")),
            }
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}
