use crate::{
    api::tenant::TenantState,
    domain::snapshot::{SnapshotInput, SnapshotPatch, validate_snapshot_urn},
    error::AndromedaError,
    http::error::problem::NgsiLdError,
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use urn::Urn;

pub async fn create(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Json(input): Json<SnapshotInput>) -> Result<Response, NgsiLdError> {
    let snapshot = state.snapshots.create(input).await.map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/snapshots/{}", snapshot.id);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Location",
        HeaderValue::from_str(&location).map_err(|e| {
            AndromedaError::internal(format!("Invalid snapshot ID for header: {}", snapshot.id))
                .with_source(e)
                .with_instance(uri.clone())
        })?,
    );

    let body = serde_json::to_value(&snapshot).map_err(|e| AndromedaError::internal("Failed to serialize snapshot").with_source(e).with_instance(uri))?;
    Ok((StatusCode::CREATED, headers, Json(body)).into_response())
}

pub async fn get_status(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(snapshot_id): Path<String>) -> Result<Response, NgsiLdError> {
    let snapshot_id = parse_snapshot_id(&snapshot_id, &uri)?;
    let snapshot = state.snapshots.get(&snapshot_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    let body = serde_json::to_value(&snapshot).map_err(|e| AndromedaError::internal("Failed to serialize snapshot").with_source(e).with_instance(uri))?;
    Ok((StatusCode::OK, Json(body)).into_response())
}

pub async fn update_status(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(snapshot_id): Path<String>,
    Json(patch): Json<SnapshotPatch>,
) -> Result<Response, NgsiLdError> {
    let snapshot_id = parse_snapshot_id(&snapshot_id, &uri)?;
    let updated = state.snapshots.update(&snapshot_id, patch).await.map_err(|e| e.with_instance(uri.clone()))?;

    let body = serde_json::to_value(&updated).map_err(|e| AndromedaError::internal("Failed to serialize snapshot").with_source(e).with_instance(uri))?;
    Ok((StatusCode::OK, Json(body)).into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(snapshot_id): Path<String>) -> Result<Response, NgsiLdError> {
    let snapshot_id = parse_snapshot_id(&snapshot_id, &uri)?;
    state.snapshots.delete(&snapshot_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn purge(TenantState(state): TenantState, OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    state.snapshots.purge().await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn clone_snapshot(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(snapshot_id): Path<String>) -> Result<Response, NgsiLdError> {
    let snapshot_id = parse_snapshot_id(&snapshot_id, &uri)?;
    let cloned = state.snapshots.clone_snapshot(&snapshot_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/snapshots/{}", cloned.id);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Location",
        HeaderValue::from_str(&location).map_err(|e| {
            AndromedaError::internal(format!("Invalid snapshot ID for header: {}", cloned.id))
                .with_source(e)
                .with_instance(uri.clone())
        })?,
    );

    let body = serde_json::to_value(&cloned).map_err(|e| AndromedaError::internal("Failed to serialize snapshot").with_source(e).with_instance(uri))?;
    Ok((StatusCode::CREATED, headers, Json(body)).into_response())
}

fn parse_snapshot_id(snapshot_id: &str, uri: &Uri) -> Result<Urn, NgsiLdError> {
    let id = snapshot_id
        .parse::<Urn>()
        .map_err(|e| AndromedaError::bad_request_data(format!("Invalid snapshot id URN: {}", e)).with_instance(uri.clone()))?;
    validate_snapshot_urn(&id).map_err(|e| AndromedaError::bad_request_data(format!("Invalid snapshot id: {}", e)).with_instance(uri.clone()))?;
    Ok(id)
}
