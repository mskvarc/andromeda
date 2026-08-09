//! Handlers for the `/csourceRegistrations` endpoints.

use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    error::{AndromedaError, ResultInstanceExt},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    prefer::PreferHeader,
    query::ValidatedQuery,
    typed_path::TypedPath,
};
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    pagination::{PaginationMediaType, PaginationStyle, append_pagination_links},
    problem_response::ProblemResponse,
    render::start_rendering,
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_render::{registration::RegistrationRenderer, response::ResponsePayload};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, csr::CsrQueryParameters};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/csourceRegistrations -- Register a new context source.
pub async fn create(
    andromeda_http_extraction::tenant_state::ProvisionableTenantState(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csr;
    let context = context.merge(body_context);

    let id = service.create(payload, &context).await.with_instance(&uri)?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/csourceRegistrations/{}", id))]).into_response())
}

/// GET /ngsi-ld/v1/csourceRegistrations/{registration_id} -- Retrieve a context source registration.
pub async fn get(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    TypedPath(registration_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let csr = service.get(&registration_id, &context).await.with_instance(&uri)?;

    let csr = RegistrationRenderer::new(csr).with_sys_attrs(options.sys_attrs).render();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(csr))
        .render()
        .with_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// GET /ngsi-ld/v1/csourceRegistrations -- Query context source registrations.
pub async fn query(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let limit = i64::from(params.effective_limit());
    let offset = i64::from(params.effective_offset());

    let (csrs, total) = service.query(&params, &context).await.with_instance(&uri)?;

    let csrs = csrs
        .into_iter()
        .map(|csr| RegistrationRenderer::new(csr).with_sys_attrs(options.sys_attrs).render())
        .collect::<Vec<_>>();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(csrs))
        .render()
        .with_instance(&uri)?;
    let total_count = usize::try_from(total)
        .map_err(|error| AndromedaError::internal(format!("registration count does not fit the response platform: {error}")))
        .with_instance(&uri)?;
    let pagination_total = i64::try_from(total)
        .map_err(|error| AndromedaError::internal(format!("registration count does not fit offset pagination: {error}")))
        .with_instance(&uri)?;
    let response = NgsiLdCollectionResponse::ok(rendered).with_total_count(total_count);

    let mut response = response.into_response();
    append_pagination_links(
        &mut response,
        &uri,
        limit,
        offset,
        pagination_total,
        PaginationStyle::OffsetFull,
        PaginationMediaType::None,
    );
    Ok(response)
}

/// PATCH /ngsi-ld/v1/csourceRegistrations/{registration_id} -- Update a context source registration.
pub async fn update(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(registration_id): TypedPath<Uri>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csr;
    let context = context.merge(body_context);
    service.update(&registration_id, payload, &context).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceRegistrations/{registration_id} -- Delete a context source registration.
pub async fn delete(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(registration_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csr;
    service.delete(&registration_id).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
