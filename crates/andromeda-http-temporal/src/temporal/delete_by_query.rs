//! HTTP handling for the temporal history delete-by-query (the
//! `temporal_delete_by_query` extension; see
//! `docs/extensions/temporal-delete-by-query.md`).
//!
//! The route is installed only when the extension is enabled, so reaching this
//! handler is itself the gate. The operation is local-only — never forwarded —
//! so unlike the per-id temporal delete it consults no registration and runs
//! entirely against this broker's history. A `dryRun` answers `200` with the
//! would-delete count; a real delete answers `204`; an over-the-bound delete
//! answers `403 TooManyResults` off a pre-count, having removed nothing.

use crate::{error::TemporalResultExt, state::TemporalTenantState};
use andromeda_http_extraction::{context::ContextUrl, context_target::ExistingTargetedTenantState, temporal_delete_query::TemporalDeleteQuery};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_temporal::delete_by_query::TemporalDeleteOutcome;
use axum::{
    Json,
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// The body of a dry-run answer: how many history instances a real delete would
/// remove.
#[derive(Debug, Serialize)]
struct WouldDelete {
    /// The count the delete would have removed.
    deleted: u64,
}

pub async fn delete_by_query(
    ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TemporalTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TemporalDeleteQuery(params): TemporalDeleteQuery,
) -> Result<Response, ProblemResponse> {
    let context = context.into_inner();
    let outcome = state.temporal.delete_by_query(&target, &params, &context).await.with_instance(&uri)?;
    match outcome {
        // A dry run removed nothing and reports what it would have; the count is
        // the vendor extension's own response shape, not an NGSI-LD collection.
        TemporalDeleteOutcome::DryRun { would_delete } => Ok((StatusCode::OK, Json(WouldDelete { deleted: would_delete })).into_response()),
        TemporalDeleteOutcome::Deleted { .. } => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}
