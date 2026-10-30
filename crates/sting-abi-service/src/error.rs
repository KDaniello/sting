//! The unified `{"error": {"code": "...", "message": "..."}}` response
//! shape on every unsuccessful response
//! (`docs/domains/service.md`) plus a JSON
//! extractor/responder (`AppJson`) that routes every failure — malformed
//! body, oversized body, or a real extraction error — through that same
//! shape, rather than letting `axum`'s own default rejection bodies leak
//! through unstructured.

use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

use sting_abi_core::ExtractError;
use sting_abi_pipeline::PipelineError;

/// Every documented error code from
/// `docs/domains/service.md`
/// (`unsupported_language`, `compilation_failed`, `timeout`,
/// `payload_too_large`, `rate_limited`, `internal_error`), plus one
/// pragmatic addition not in that original enumeration:
/// [`Self::InvalidRequest`] (`invalid_request`) — malformed/unparseable
/// JSON bodies and unknown `lang` values are real failure modes the spec's
/// code list didn't name explicitly (it was focused on extraction-specific
/// failures); reusing `internal_error` for a client mistake would be
/// actively misleading (that code means "our bug", not "your request"),
/// so this adds a named code rather than silently misclassifying.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// No extractor is registered/resolvable for the requested language.
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),
    /// The compiler ran and produced a real compile error.
    #[error("compilation failed: {0}")]
    CompilationFailed(String),
    /// Extraction did not finish within the configured timeout.
    #[error("extraction timed out")]
    Timeout,
    /// The request body exceeded the configured size limit.
    #[error("payload too large")]
    PayloadTooLarge,
    /// The requesting IP has exceeded its rate-limit budget.
    #[error("rate limited")]
    RateLimited,
    /// The request itself was malformed (bad JSON, unknown `lang`, batch
    /// too large, ...) — a client mistake, not a server failure.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// Something failed that should not be possible from client input
    /// alone — logged server-side with full detail; the client only ever
    /// sees a generic message, never leaking internal paths/state.
    #[error("internal error: {0}")]
    Internal(String),
    /// `GET /v1/schema/{address}` found no verified source for this
    /// address — an honest structural error
    /// (`docs/domains/service.md`), never
    /// an empty `200`.
    #[error("no verified source for this address")]
    NoVerifiedSource,
    /// Resolving on-chain code failed (network error, provider outage,
    /// or the address genuinely doesn't exist on-chain) — a `502`-style
    /// upstream failure, not this service's own bug (`internal_error`
    /// would be misleading) and not necessarily the client's mistake
    /// either (`invalid_request` would overclaim certainty about whose
    /// fault it is).
    #[error("failed to resolve on-chain code: {0}")]
    OnChainResolutionFailed(String),
    /// The `/v1/verify`/`/v1/schema/{address}` machinery isn't configured
    /// on this deployment (e.g. the platform cache directory couldn't be
    /// determined at startup) — a deployment gap, not a per-request bug.
    #[error("verification is not available on this deployment")]
    RegistryUnavailable,
    /// `POST /v1/probe`'s Tier 3 machinery (method-id dictionary,
    /// `libemulator`, or the on-chain corpus resolver) isn't configured on
    /// this deployment — same "deployment gap, not per-request bug"
    /// reasoning as [`Self::RegistryUnavailable`].
    #[error("Tier 3 probing is not available on this deployment")]
    Tier3Unavailable,
    /// No operation dictionary is configured on this deployment, so no message can be read.
    #[error("decoding is not available on this deployment")]
    DecodeUnavailable,
    /// The request presented no key, or one this deployment does not know.
    #[error("unauthorized")]
    Unauthorized,
    /// The key is known and has spent its period's budget.
    #[error("quota exhausted")]
    QuotaExhausted,
    /// The answer would be larger than this deployment sends.
    #[error("result too large")]
    ResultTooLarge,
    /// A real TVM probe call failed (malformed on-chain code/data, most
    /// likely) — a `502`-style upstream failure, mirroring
    /// [`Self::OnChainResolutionFailed`]'s reasoning.
    #[error("Tier 3 probe failed: {0}")]
    ProbeFailed(String),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::UnsupportedLanguage(msg) => {
                (StatusCode::BAD_REQUEST, "unsupported_language", msg.clone())
            }
            Self::CompilationFailed(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "compilation_failed",
                msg.clone(),
            ),
            Self::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "timeout",
                "extraction did not finish within the configured timeout".to_string(),
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request body exceeds the configured size limit".to_string(),
            ),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many requests from this client, try again later".to_string(),
            ),
            Self::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, "invalid_request", msg.clone()),
            Self::Internal(msg) => {
                tracing::error!(error = %msg, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal error".to_string(),
                )
            }
            Self::NoVerifiedSource => (
                StatusCode::NOT_FOUND,
                "no_verified_source",
                "no verified source is on file for this address".to_string(),
            ),
            Self::OnChainResolutionFailed(msg) => (
                StatusCode::BAD_GATEWAY,
                "onchain_resolution_failed",
                msg.clone(),
            ),
            Self::RegistryUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "registry_unavailable",
                "verification is not available on this deployment".to_string(),
            ),
            Self::Tier3Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "tier3_unavailable",
                "Tier 3 probing is not available on this deployment".to_string(),
            ),
            Self::ProbeFailed(msg) => (StatusCode::BAD_GATEWAY, "probe_failed", msg.clone()),
            Self::DecodeUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "decode_unavailable",
                "decoding is not available on this deployment".to_string(),
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "this deployment requires a key in X-Api-Key".to_string(),
            ),
            Self::QuotaExhausted => (
                StatusCode::TOO_MANY_REQUESTS,
                "quota_exhausted",
                "this key has spent its quota for the current period".to_string(),
            ),
            Self::ResultTooLarge => (
                StatusCode::INSUFFICIENT_STORAGE,
                "result_too_large",
                "the answer exceeds the configured result size limit".to_string(),
            ),
        }
    }

    /// A single `"code: message"` line — used by the batch endpoint, where
    /// one item's failure is a plain string field
    /// (`docs/domains/abi-ir.md`), not a full HTTP
    /// response of its own.
    pub(crate) fn batch_message(&self) -> String {
        let (_, code, message) = self.parts();
        format!("{code}: {message}")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        (
            status,
            AppJson(serde_json::json!({"error": {"code": code, "message": message}})),
        )
            .into_response()
    }
}

impl From<PipelineError> for ApiError {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::UnknownLanguage(path) => Self::UnsupportedLanguage(format!(
                "could not determine the source language of {} from its file extension",
                path.display()
            )),
            PipelineError::NoExtractorForLanguage(lang) => {
                Self::UnsupportedLanguage(format!("no extractor registered for {lang:?}"))
            }
            PipelineError::Extract(ExtractError::Failed {
                message, source, ..
            }) => {
                // Downcasts through the type-erased `source` to tell a real
                // compiler timeout apart from an ordinary compile error —
                // `ExtractError::Failed` only has one shape for both
                // (`sting-abi-core` doesn't know either compiler's
                // concrete error type by design, see that crate's own
                // `ExtractError` doc comment), but this crate *does*
                // already depend on `sting-tolkc`/`sting-abi-tact`
                // directly (it constructs their compilers), so the
                // downcast is always available here even though it isn't
                // in `sting-abi-core` itself.
                let is_timeout = source.as_deref().is_some_and(|err| {
                    err.downcast_ref::<sting_tolkc::TolkcError>()
                        .is_some_and(|e| matches!(e, sting_tolkc::TolkcError::Timeout { .. }))
                        || err
                            .downcast_ref::<sting_abi_tact::TactcError>()
                            .is_some_and(|e| {
                                matches!(e, sting_abi_tact::TactcError::Timeout { .. })
                            })
                });
                if is_timeout {
                    Self::Timeout
                } else {
                    Self::CompilationFailed(message)
                }
            }
        }
    }
}

impl From<sting_abi_dictionary::ProbeError> for ApiError {
    fn from(err: sting_abi_dictionary::ProbeError) -> Self {
        Self::ProbeFailed(err.to_string())
    }
}

impl From<sting_abi_registry::RegistryError> for ApiError {
    fn from(err: sting_abi_registry::RegistryError) -> Self {
        use sting_abi_registry::RegistryError;
        match err {
            RegistryError::Corpus { target, source } => {
                Self::OnChainResolutionFailed(format!("{target:?}: {source}"))
            }
            RegistryError::InvalidBase64 { target, source } => Self::OnChainResolutionFailed(
                format!("{target:?}: on-chain code was not valid base64: {source}"),
            ),
            RegistryError::NoBytecodeSourceForLanguage(lang) => {
                Self::UnsupportedLanguage(format!("verification is not supported for {lang:?} yet"))
            }
            RegistryError::Compile(compile_err) => Self::CompilationFailed(compile_err.to_string()),
            RegistryError::Extract(pipeline_err) => pipeline_err.into(),
        }
    }
}

/// A `Json<T>`-alike whose *rejections* (malformed body, oversized body,
/// wrong content type) also serialize through [`ApiError`]'s unified
/// `{"error": {...}}` shape — plain `axum::Json<T>` used directly as an
/// extractor would instead short-circuit straight to axum's own default,
/// differently-shaped rejection body, breaking the rule that every
/// unsuccessful answer has one structure.
pub struct AppJson<T>(pub T);

impl<T: Serialize> IntoResponse for AppJson<T> {
    fn into_response(self) -> Response {
        Json(self.0).into_response()
    }
}

impl<T, S> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => {
                // Capture the message before consuming `rejection` for its
                // status — `IntoResponse::into_response` takes `self`, so
                // both cannot be read from the same value afterward.
                let message = rejection.to_string();
                let status = rejection.into_response().status();
                if status == StatusCode::PAYLOAD_TOO_LARGE {
                    Err(ApiError::PayloadTooLarge)
                } else {
                    Err(ApiError::InvalidRequest(message))
                }
            }
        }
    }
}
