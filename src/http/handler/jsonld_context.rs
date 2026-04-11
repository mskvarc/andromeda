//! Handlers for the `/jsonldContexts` endpoints.

use crate::{
    api::tenant::TenantState,
    domain::jsonld_context::ContextKind,
    error::AndromedaError,
    http::{
        error::problem::NgsiLdError,
        extractor::query::ValidatedQuery,
        parameter::jsonld_context::{DeleteParams, ListParams, ServeParams},
    },
    jsonld::core_context::is_ngsi_ld_core_context_url,
    repository::jsonld_context::DetailLevel,
};
use axum::{
    Json,
    body::Bytes,
    extract::{OriginalUri, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/jsonldContexts -- Create a hosted JSON-LD context.
pub async fn create(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Bytes) -> Result<Response, NgsiLdError> {
    let service = &state.contexts;
    let payload: Value =
        serde_json::from_slice(&body).map_err(|_| AndromedaError::invalid_request("Request body is not valid JSON").with_instance(uri.clone()))?;

    let base_url = headers.get("host").and_then(|v| v.to_str().ok()).map(|h| format!("http://{}", h));

    let id = service.create(payload, base_url.as_deref()).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/jsonldContexts/{}", id))]).into_response())
}

/// GET /ngsi-ld/v1/jsonldContexts -- Query available JSON-LD contexts.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.contexts;
    let kind = params
        .kind
        .map(|k| {
            k.parse::<ContextKind>()
                .map_err(|_| crate::error::AndromedaError::bad_request_data(format!("Invalid kind: {}. Must be one of: Cached, Hosted, ImplicitlyCreated", k)))
        })
        .transpose()
        .map_err(|e| e.with_instance(uri.clone()))?;

    let detail_level = if params.details.unwrap_or(false) {
        DetailLevel::Full
    } else {
        DetailLevel::Summary
    };

    let result = service.list(kind, detail_level).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// GET /ngsi-ld/v1/jsonldContexts/{context_id} -- Retrieve a specific JSON-LD context.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(context_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ServeParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.contexts;
    let detail_level = if params.details.unwrap_or(false) {
        DetailLevel::Full
    } else {
        DetailLevel::Summary
    };

    let result = service.get(&context_id, detail_level).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// DELETE /ngsi-ld/v1/jsonldContexts/{context_id} -- Delete a JSON-LD context.
pub async fn delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(context_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<DeleteParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.contexts;
    let reload = params.reload.unwrap_or(false);

    // Check if attempting to delete a core context
    if is_ngsi_ld_core_context_url(&context_id) {
        if reload {
            // Core contexts are embedded; reload is a no-op success.
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        return Err(NgsiLdError::from(
            AndromedaError::bad_request_data("Cannot delete a core context").with_instance(uri.clone()),
        ));
    }

    service.delete(&context_id, reload).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
