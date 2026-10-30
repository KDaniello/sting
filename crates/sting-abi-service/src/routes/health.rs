//! `GET /v1/health`, `GET /v1/version` — standard public-service hygiene
//! (`docs/domains/service.md`), never
//! rate-limited (see `crate::rate_limit::rate_limit_middleware`'s doc
//! comment).

use axum::Json;
use serde_json::{Value, json};

/// Always `200 {"status": "ok"}` if the process is up and answering HTTP
/// at all — deliberately does not probe the Tolk/Tact compilers (a slow or
/// broken compiler is a per-request `compilation_failed`/`timeout`, not a
/// reason for an orchestrator to consider the whole service unhealthy and
/// restart it).
pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// This service's own crate version plus the `ContractSchema` IR version
/// it produces — a caller embedding this service needs both to know
/// whether it can parse what comes back.
pub async fn version() -> Json<Value> {
    Json(json!({
        "service": env!("CARGO_PKG_VERSION"),
        "ir_schema_version": sting_abi_core::IR_SCHEMA_VERSION,
    }))
}
