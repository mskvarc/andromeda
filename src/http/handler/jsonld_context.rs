//! Handlers for the `/jsonldContexts` endpoints.

use crate::{
    api::tenant::TenantState,
    error::{AndromedaError, ResultInstanceExt},
    http::origin::request_origin,
};
use andromeda_context_registry::{detail_level::ContextDetailLevel, removal::ContextRemoval};
use andromeda_http_extraction::{query::ValidatedQuery, typed_path::TypedPath};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_request::jsonld_context::{DeleteParams, ListParams, ServeParams};
use axum::{
    Json,
    body::Bytes,
    extract::OriginalUri,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/jsonldContexts -- Create a hosted JSON-LD context.
pub async fn create(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ProblemResponse> {
    let service = &state.contexts;
    let payload: Value = serde_json::from_slice(&body).map_err(|_| AndromedaError::malformed_request("Request body is not valid JSON").with_instance(&uri))?;

    let base_url = request_origin(&headers).map_err(|error| error.with_instance(&uri))?;

    let id = service.create(payload, base_url.as_ref()).await.with_instance(&uri)?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/jsonldContexts/{}", id))]).into_response())
}

/// GET /ngsi-ld/v1/jsonldContexts -- Query available JSON-LD contexts.
pub async fn query(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.contexts;
    let kind = params.kind;

    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };

    let result = service.list(kind, detail_level).await.with_instance(&uri)?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// GET /ngsi-ld/v1/jsonldContexts/{context_id} -- Retrieve a specific JSON-LD context.
pub async fn get(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(context_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<ServeParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.contexts;
    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };

    let result = service.get(&context_id, detail_level).await.with_instance(&uri)?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// DELETE /ngsi-ld/v1/jsonldContexts/{context_id} -- Delete a JSON-LD context.
pub async fn delete(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(context_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<DeleteParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.contexts;
    let removal = match params.reload {
        Some(true) => ContextRemoval::Reload,
        Some(false) | None => ContextRemoval::Delete,
    };
    service.remove(&context_id, removal).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
