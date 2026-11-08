//! Handlers for the `/jsonldContexts` endpoints.

use crate::{
    context_id_segment::ContextId,
    error::ContextRegistryResultExt,
    inadmissible_body::InadmissibleBody,
    served_document::served_response,
    state::ContextRegistryTenantState,
};
use andromeda_context_registry::{detail_level::ContextDetailLevel, removal::ContextRemoval};
use andromeda_http_extraction::{
    api_root::ApiRoot,
    body_preconditions::BodyPreconditions,
    not_json_document::NotJsonDocument,
    pagination_policy::RequestPagination,
    query::ValidatedQuery,
    request_payload::RequestPayload,
    resource_location::{ResourceCollection, ResourceLocator},
    tenant_state::ExistingTenantState,
    typed_path::TypedPath,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    pagination::{PageOutcome, append_page_links},
    parameter_failure::classify_parameter_error,
    problem_response::ProblemResponse,
    request_refusal::RequestRefusal,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use andromeda_ngsi_ld_request::{
    empty::EmptyParams,
    jsonld_context::{CONTEXT_PAGE_SIZE, DeleteParams, ListParams, ServeParams},
    pagination::{
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
    },
};
use axum::{
    Json,
    extract::OriginalUri,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::{HeaderMapExt, IfNoneMatch};
use serde_json::value::RawValue;

/// Create a hosted or cached JSON-LD `@context` (clause 5.13.2).
///
/// Clause 6.3.4 applies its POST preconditions here as to every other
/// body-taking operation: the [`BodyPreconditions`] extractor refuses a
/// request that declares no `Content-Length` with the bare 411 the clause
/// requires, and one whose `Content-Type` is not admitted with the bare 415.
/// It is listed first because the preconditions are properties of the incoming
/// HTTP request alone, so they hold whether or not the addressed tenant
/// resolves.
///
/// The entry's `url` member is the absolute URL it is served under, built on
/// the request's `apiRoot`; the `Location` is the local relative path clause
/// 6.29.3.1 prescribes, below the configured `apiRoot`'s prefix. The operation defines no query parameter, so any is the
/// clause 6.3.20 `InvalidRequest`, read before the tenant is resolved.
pub(crate) async fn create(
    _preconditions: BodyPreconditions,
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    locator: ResourceLocator,
    // Counted within the JSON value limit as it is read, so a payload of too
    // many values costs its bytes and nothing more.
    RequestPayload(body): RequestPayload,
) -> Result<Response, ProblemResponse> {
    let payload = serde_json::from_slice::<&RawValue>(&body)
        .map_err(|source| InadmissibleBody::NotJson(NotJsonDocument(source)))
        .and_then(|raw| JsonLiteral::admit_raw(raw).map_err(InadmissibleBody::of))
        // clone: the request URI is still needed by the steps after this refusal
        .map_err(|refusal| refusal.into_problem(uri.clone()))?;
    // Clause 5.13.2 records the absolute URL the entry is served under, which a
    // request naming no authority gives nothing to build on (IETF RFC 9112
    // section 3.2 makes such an HTTP/1.1 request malformed).
    let urls = state.hosting.urls(&api_root).ok_or_else(|| {
        ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, "the request names no authority to serve the @context under").with_instance(uri.clone())
    })?;
    let identifier = state.hosting.contexts().create(&payload, &urls).await.with_context_registry_instance(&uri)?;
    Ok(locator.locate(StatusCode::CREATED.into_response(), ResourceCollection::JsonldContexts, &identifier))
}

/// List the stored `@context`s (clause 5.13.3).
///
/// Clause 5.13.3.4 answers with every stored `@context` that matches the
/// filter, so the collection is paged by the broker's pagination policy like
/// every other one (clauses 4.12 and 5.5.9): a page in local-identifier order,
/// with the clause 6.3.10 `Link` pointers while other pages exist.
pub(crate) async fn query(
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    RequestPagination {
        policy: pagination,
        context,
        query,
    }: RequestPagination,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };
    let window = pagination
        .window(
            &params.page,
            PageRequest {
                size: CONTEXT_PAGE_SIZE,
                order: CollectionOrder::Identifier,
                count: CountPolicy::Omit,
                pointers: PagePointers::OverRequestUri,
                context: &context,
                query: &query,
            },
        )
        .map_err(|error| classify_parameter_error(&error).with_instance(uri.clone()))?;
    let page = state
        .hosting
        .contexts()
        .list(params.kind, detail_level, &window)
        .await
        .with_context_registry_instance(&uri)?;
    let mut response = (StatusCode::OK, Json(page.contexts)).into_response();
    append_page_links(
        &mut response,
        &uri,
        &context,
        &window,
        PageOutcome {
            total: None,
            reach: page.reach,
            boundary: page.boundary,
        },
        ResponseMediaType::Json,
    );
    Ok(response)
}

/// Serve a stored `@context` or its metadata (clause 5.13.4).
///
/// The tenant comes from `NGSILD-Tenant` alone, as for every other operation
/// (clauses 4.14 and 6.3.14), and is the default tenant without the header; a
/// tenant that does not exist is `NonexistentTenant` (clause 5.5.10). The
/// parameters are read before the tenant is resolved, so one the operation
/// does not define is the clause 6.3.20 `InvalidRequest` for every tenant.
/// `If-None-Match` is evaluated only once the tenant, the identifier and the
/// entry's kind resolved (RFC 7232 section 5). A HEAD request runs this handler
/// and loses the body on the way out, so it answers with exactly the headers a
/// GET does. The route marks every answer, errors included, as varying on the
/// tenant header ([`crate::served_document::vary_on_tenant`]).
pub(crate) async fn get(
    ValidatedQuery(params): ValidatedQuery<ServeParams>,
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    request_headers: HeaderMap,
    TypedPath(ContextId(requested)): TypedPath<ContextId>,
) -> Result<Response, ProblemResponse> {
    let detail_level = match params.details {
        Some(true) => ContextDetailLevel::Full,
        Some(false) | None => ContextDetailLevel::Summary,
    };
    let served = state
        .hosting
        .contexts()
        .get(&requested, detail_level)
        .await
        .with_context_registry_instance(&uri)?;
    let if_none_match = request_headers.typed_get::<IfNoneMatch>();
    served_response(served, if_none_match.as_ref()).map_err(|report| report.with_instance(uri))
}

/// Delete a stored `@context`, or reload a cached one from its source when
/// `reload=true` (clause 5.13.5); answers 204.
pub(crate) async fn delete(
    ExistingTenantState(state): ContextRegistryTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(ContextId(requested)): TypedPath<ContextId>,
    ValidatedQuery(params): ValidatedQuery<DeleteParams>,
) -> Result<Response, ProblemResponse> {
    let removal = match params.reload {
        Some(true) => ContextRemoval::Reload,
        Some(false) | None => ContextRemoval::Delete,
    };
    state
        .hosting
        .contexts()
        .remove(&requested, removal)
        .await
        .with_context_registry_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
