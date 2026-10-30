//! `GET /v1/schema/{address}` — reads from the persisted verification
//! store `POST /v1/verify` writes to; does **not** itself trigger a fresh
//! on-chain fetch/recompile (that would require a source to recompile
//! against, which a bare `GET` by address doesn't have) — a `404
//! no_verified_source`, not an empty `200`, when nothing is on file.

use axum::extract::{Path, State};
use axum::response::IntoResponse;

use crate::error::{ApiError, AppJson};
use crate::state::AppState;

pub async fn schema_by_address(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let record = state.verified_store.get(&address).map_err(|err| {
        ApiError::Internal(format!("failed to read the verified-schema store: {err}"))
    })?;
    match record {
        Some(record) => Ok(AppJson(record.schema)),
        None => Err(ApiError::NoVerifiedSource),
    }
}
