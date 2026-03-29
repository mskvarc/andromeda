use crate::{
    api::{error::ApiError, tenant::TenantState},
    domain::snapshot::{SnapshotInput, SnapshotPatch},
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

pub async fn create(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Json(input): Json<SnapshotInput>) -> Result<Response, ApiError> {
    let snapshot = state.snapshots.create(input).await.map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/snapshots/{}", snapshot.id);
    let mut headers = HeaderMap::new();
    headers.insert("Location", HeaderValue::from_str(&location).unwrap());

    Ok((StatusCode::CREATED, headers, Json(serde_json::to_value(&snapshot).unwrap_or_default())).into_response())
}

pub async fn get_status(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(snapshot_id): Path<String>) -> Result<Response, ApiError> {
    let snapshot = state.snapshots.get(&snapshot_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::OK, Json(serde_json::to_value(&snapshot).unwrap_or_default())).into_response())
}

pub async fn update_status(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(snapshot_id): Path<String>,
    Json(patch): Json<SnapshotPatch>,
) -> Result<Response, ApiError> {
    let updated = state.snapshots.update(&snapshot_id, patch).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::OK, Json(serde_json::to_value(&updated).unwrap_or_default())).into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(snapshot_id): Path<String>) -> Result<Response, ApiError> {
    state.snapshots.delete(&snapshot_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn purge(TenantState(state): TenantState, OriginalUri(uri): OriginalUri) -> Result<Response, ApiError> {
    state.snapshots.purge().await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn clone_snapshot(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(snapshot_id): Path<String>) -> Result<Response, ApiError> {
    let cloned = state.snapshots.clone_snapshot(&snapshot_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/snapshots/{}", cloned.id);
    let mut headers = HeaderMap::new();
    headers.insert("Location", HeaderValue::from_str(&location).unwrap());

    Ok((StatusCode::CREATED, headers, Json(serde_json::to_value(&cloned).unwrap_or_default())).into_response())
}
