//! Handlers for the `/jsonldContexts` endpoints.

use crate::{
    error::ContextRegistryResultExt,
    hosted_context_url::hosted_context_path,
    serve_tenant::serve_tenant,
    served_document::served_response,
    state::{ContextRegistryRouteState, ContextRegistryTenantState},
};
use andromeda_context_registry::{detail_level::ContextDetailLevel, identifier::ContextIdentifier, removal::ContextRemoval};
use andromeda_http_extraction::{
    api_root::ApiRoot,
    content_length_precondition::DeclaredBodyLength,
    query::ValidatedQuery,
    request_payload::RequestPayload,
    tenant::TenantHeader,
    tenant_state::{ExistingTenantState, TenantStateResolver},
};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    headers::with_location,
    internal_failure_summary::InternalFailureSummary,
    problem_response::ProblemResponse,
};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::{
    empty::EmptyParams,
    jsonld_context::{DeleteParams, ListParams, ServeParams},
};
use andromeda_tenancy::creation_policy::TenantCreationPolicy;
use axum::{
    Json,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::{HeaderMapExt, IfNoneMatch};
use serde_json::Value;

/// Create a hosted or cached JSON-LD `@context` (clause 5.13.2).
///
/// Clause 6.3.4 applies its POST preconditions here as to every other
/// body-taking operation: the [`DeclaredBodyLength`] extractor refuses a
/// request that declares no `Content-Length` with the bare 411 the clause
/// requires. It is listed first because the precondition is a property of the
/// incoming HTTP request alone, so it holds whether or not the addressed
/// tenant resolves.
///
/// The entry's `url` member is the absolute URL it is served under, built on
/// the request's `apiRoot`; the `Location` stays the local relative path clause
/// 6.29.3.1 prescribes. The operation defines no query parameter, so any is the
/// clause 6.3.20 `InvalidRequest`, read before the tenant is resolved.
pub(crate) async fn create(
    _body_length: DeclaredBodyLength,
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    RequestPayload(body): RequestPayload,
) -> Result<Response, ProblemResponse> {
    let payload: Value = serde_json::from_slice(&body).map_err(|error| {
        ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, format!("request body is not valid JSON: {error}")).with_instance(uri.clone())
    })?;
    // Clause 5.13.2 records the absolute URL the entry is served under, which a
    // request naming no authority gives nothing to build on (IETF RFC 9112
    // section 3.2 makes such an HTTP/1.1 request malformed).
    let urls = state.hosting.urls(&api_root).ok_or_else(|| {
        ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, "the request names no authority to serve the @context under").with_instance(uri.clone())
    })?;
    let identifier = state.hosting.contexts().create(payload, &urls).await.with_context_registry_instance(&uri)?;
    let location =
        hosted_context_path(&identifier).map_err(|error| ClassifiedReport::internal(&error, InternalFailureSummary::RESPONSE_ENCODING).with_instance(uri))?;
    Ok(with_location(StatusCode::CREATED.into_response(), location))
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
    let result = state
        .hosting
        .contexts()
        .list(params.kind, detail_level)
        .await
        .with_context_registry_instance(&uri)?;
    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Serve a stored `@context` or its metadata (clause 5.13.4).
///
/// The tenant comes from `NGSILD-Tenant`, else from the `andromedaTenant`
/// parameter a URL minted for a non-default tenant's entry carries, else the
/// default tenant ([`serve_tenant`]); a tenant that does not exist is
/// `NonexistentTenant` (clause 5.5.10). `If-None-Match` is evaluated only once
/// the tenant, the identifier and the entry's kind resolved (RFC 7232 section
/// 5). A HEAD request runs this handler and loses the body on the way out, so
/// it answers with exactly the headers a GET does. The route marks every
/// answer, errors included, as varying on the tenant header
/// ([`crate::served_document::vary_on_tenant`]).
pub(crate) async fn get(
    State(route_state): State<ContextRegistryRouteState>,
    OriginalUri(uri): OriginalUri,
    TenantHeader(header): TenantHeader,
    request_headers: HeaderMap,
    Path(context_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ServeParams>,
) -> Result<Response, ProblemResponse> {
    let tenant = serve_tenant(header, params.andromeda_tenant)
        .map_err(|conflict| ClassifiedReport::public(NgsiLdErrorType::BadRequestData, conflict.to_string()).with_instance(uri.clone()))?;
    let state = route_state.resolve_tenant(&tenant, TenantCreationPolicy::MustExist, &uri).await?;
    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };
    let requested = ContextIdentifier::from(context_id.as_str());
    let served = state
        .hosting
        .contexts()
        .get(&requested, detail_level)
        .await
        .with_context_registry_instance(&uri)?;
    let if_none_match = request_headers.typed_get::<IfNoneMatch>();
    served_response(served, if_none_match.as_ref()).map_err(|report| report.with_instance(uri))
}

pub(crate) async fn delete(
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    Path(context_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<DeleteParams>,
) -> Result<Response, ProblemResponse> {
    let removal = match params.reload {
        Some(true) => ContextRemoval::Reload,
        Some(false) | None => ContextRemoval::Delete,
    };
    let requested = ContextIdentifier::from(context_id.as_str());
    state
        .hosting
        .contexts()
        .remove(&requested, removal)
        .await
        .with_context_registry_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
