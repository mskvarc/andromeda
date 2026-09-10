//! Handlers for the `/jsonldContexts` endpoints.

use crate::{
    error::{ContextRegistryResultExt, origin_problem},
    state::ContextRegistryTenantState,
};
use andromeda_context_registry::{detail_level::ContextDetailLevel, removal::ContextRemoval};
use andromeda_http_extraction::{
    content_length_precondition::DeclaredBodyLength,
    origin::request_origin,
    query::ValidatedQuery,
    tenant_state::ExistingTenantState,
    typed_path::TypedPath,
};
use andromeda_http_response::{classified_report::ClassifiedReport, headers::with_location, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::jsonld_context::{DeleteParams, ListParams, ServeParams};
use axum::{
    Json,
    body::Bytes,
    extract::OriginalUri,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// Create a hosted or cached JSON-LD `@context` (clause 5.13.2).
///
/// Clause 6.3.4 applies its POST preconditions here as to every other
/// body-taking operation: the [`DeclaredBodyLength`] extractor refuses a
/// request that declares no `Content-Length` with the bare 411 the clause
/// requires. It is listed first because the precondition is a property of the
/// incoming HTTP request alone, so it holds whether or not the addressed
/// tenant resolves.
pub(crate) async fn create(
    _body_length: DeclaredBodyLength,
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ProblemResponse> {
    let payload: Value = serde_json::from_slice(&body).map_err(|error| {
        ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, format!("Request body is not valid JSON: {error}")).with_instance(uri.clone())
    })?;
    let base_url = request_origin(&headers).map_err(|error| origin_problem(&error, &uri))?;
    let identifier = state.contexts.create(payload, base_url.as_ref()).await.with_context_registry_instance(&uri)?;
    Ok(with_location(
        StatusCode::CREATED.into_response(),
        format!("/ngsi-ld/v1/jsonldContexts/{identifier}"),
    ))
}

pub(crate) async fn query(
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };
    let result = state.contexts.list(params.kind, detail_level).await.with_context_registry_instance(&uri)?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

pub(crate) async fn get(
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(context_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<ServeParams>,
) -> Result<Response, ProblemResponse> {
    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };
    let result = state.contexts.get(&context_id, detail_level).await.with_context_registry_instance(&uri)?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

pub(crate) async fn delete(
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(context_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<DeleteParams>,
) -> Result<Response, ProblemResponse> {
    let removal = match params.reload {
        Some(true) => ContextRemoval::Reload,
        Some(false) | None => ContextRemoval::Delete,
    };
    state.contexts.remove(&context_id, removal).await.with_context_registry_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
