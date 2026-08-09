//! Handlers for the `/snapshots` endpoints.

use crate::{
    api::tenant::TenantState,
    error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{json_body::invalid_json_body, snapshot_purge_q::SnapshotPurgeQ, typed_path::TypedPath},
    },
    service::snapshot::PurgeOutcome,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::snapshot::{SnapshotInput, SnapshotPatch};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use axum::{
    Json,
    extract::{OriginalUri, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/snapshots -- Create a new snapshot.
pub async fn create(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    input: Result<Json<SnapshotInput>, JsonRejection>,
) -> Result<Response, NgsiLdError> {
    let Json(input) = input.map_err(|rejection| invalid_json_body(rejection, &uri, AndromedaError::snapshot_validation))?;
    let snapshot = state.snapshots.create(input).await.with_instance(&uri)?;

    let location = format!("/ngsi-ld/v1/snapshots/{}", snapshot.id);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Location",
        HeaderValue::from_str(&location)
            .map_err(|e| AndromedaError::internal(format!("Invalid snapshot ID for header: {}: {}", snapshot.id, e)).with_instance(&uri))?,
    );

    let body = serde_json::to_value(&snapshot).map_err(|e| AndromedaError::internal(format!("Failed to serialize snapshot: {}", e)).with_instance(&uri))?;
    Ok((StatusCode::CREATED, headers, Json(body)).into_response())
}

/// GET /ngsi-ld/v1/snapshots/{snapshot_id} -- Retrieve snapshot status.
pub async fn get(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, TypedPath(snapshot_id): TypedPath<Uri>) -> Result<Response, NgsiLdError> {
    let snapshot = state.snapshots.get(&snapshot_id).await.with_instance(&uri)?;

    let body = serde_json::to_value(&snapshot).map_err(|e| AndromedaError::internal(format!("Failed to serialize snapshot: {}", e)).with_instance(&uri))?;
    Ok((StatusCode::OK, Json(body)).into_response())
}

/// PATCH /ngsi-ld/v1/snapshots/{snapshot_id} -- Update snapshot status.
pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(snapshot_id): TypedPath<Uri>,
    patch: Result<Json<SnapshotPatch>, JsonRejection>,
) -> Result<Response, NgsiLdError> {
    let Json(patch) = patch.map_err(|rejection| invalid_json_body(rejection, &uri, AndromedaError::snapshot_validation))?;
    let updated = state.snapshots.update(&snapshot_id, patch).await.with_instance(&uri)?;

    let body = serde_json::to_value(&updated).map_err(|e| AndromedaError::internal(format!("Failed to serialize snapshot: {}", e)).with_instance(&uri))?;
    Ok((StatusCode::OK, Json(body)).into_response())
}

/// DELETE /ngsi-ld/v1/snapshots/{snapshot_id} -- Delete a snapshot.
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, TypedPath(snapshot_id): TypedPath<Uri>) -> Result<Response, NgsiLdError> {
    state.snapshots.delete(&snapshot_id).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/snapshots -- Purge snapshots selected by a `q` filter.
///
/// Implements ETSI GS CIM 009 v1.9.1 clauses 5.16.7 and 6.36.3.2:
///
/// - Missing or malformed `q` -> 400 BadRequestData (handled by the
///   [`SnapshotPurgeQ`] extractor).
/// - No matches -> 404 ResourceNotFound.
/// - All matches deleted -> 204 No Content.
/// - At least one deletion failed -> 207 Multi-Status with a
///   [`BatchOperationResult`] body.
pub async fn purge(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, SnapshotPurgeQ(filter): SnapshotPurgeQ) -> Result<Response, NgsiLdError> {
    let outcome = state.snapshots.purge_by_query(&filter).await.with_instance(&uri)?;

    match outcome {
        PurgeOutcome::NoMatch => Err(NgsiLdError::new(NgsiLdErrorType::ResourceNotFound, "No snapshots matched the purge query", uri)),
        PurgeOutcome::FullSuccess => Ok(StatusCode::NO_CONTENT.into_response()),
        PurgeOutcome::AllFailed { failures } => {
            let body = BatchOperationResult {
                success: Vec::new(),
                errors: failures,
            };
            Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
        }
        PurgeOutcome::Partial { successes, failures } => {
            let body = BatchOperationResult {
                success: successes,
                errors: failures,
            };
            Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
        }
    }
}

/// POST /ngsi-ld/v1/snapshots/{snapshot_id}/clone -- Clone a snapshot.
pub async fn clone(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, TypedPath(snapshot_id): TypedPath<Uri>) -> Result<Response, NgsiLdError> {
    let cloned = state.snapshots.clone_snapshot(&snapshot_id).await.with_instance(&uri)?;

    let location = format!("/ngsi-ld/v1/snapshots/{}", cloned.id);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Location",
        HeaderValue::from_str(&location)
            .map_err(|e| AndromedaError::internal(format!("Invalid snapshot ID for header: {}: {}", cloned.id, e)).with_instance(&uri))?,
    );

    let body = serde_json::to_value(&cloned).map_err(|e| AndromedaError::internal(format!("Failed to serialize snapshot: {}", e)).with_instance(&uri))?;
    Ok((StatusCode::CREATED, headers, Json(body)).into_response())
}
