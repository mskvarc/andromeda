//! Handlers for the `/csourceRegistrations` endpoints.

use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    error::ResultInstanceExt,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, prefer::PreferHeader, query::ValidatedQuery},
        response::{
            collection::NgsiLdCollectionResponse,
            pagination::{PaginationMediaType, PaginationStyle, append_pagination_links},
            render::start_rendering,
            single::NgsiLdResponse,
        },
        validation::parse_csr_id,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_render::{registration::RegistrationRenderer, response::ResponsePayload};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, csr::CsrQueryParameters};
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/csourceRegistrations -- Register a new context source.
pub async fn create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.csr;
    let context = context.merge(body_context);

    let id = service.create(payload, &context).await.with_instance(&uri)?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/csourceRegistrations/{}", id))]).into_response())
}

/// GET /ngsi-ld/v1/csourceRegistrations/{registration_id} -- Retrieve a context source registration.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(registration_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, NgsiLdError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let csr = service.get(&registration_id, &context).await.with_instance(&uri)?;

    let csr = RegistrationRenderer::new(csr).with_sys_attrs(options.sys_attrs).render();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(csr))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// GET /ngsi-ld/v1/csourceRegistrations -- Query context source registrations.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, NgsiLdError> {
    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let limit = params.effective_limit() as i64;
    let offset = params.effective_offset() as i64;

    let (csrs, total) = service.query(&params, &context).await.with_instance(&uri)?;

    let csrs = csrs
        .into_iter()
        .map(|csr| RegistrationRenderer::new(csr).with_sys_attrs(options.sys_attrs).render())
        .collect::<Vec<_>>();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Collection(csrs))
        .render()?;
    let response = NgsiLdCollectionResponse::ok(rendered).with_total_count(total as usize);

    let mut response = response.into_response();
    append_pagination_links(
        &mut response,
        &uri,
        limit,
        offset,
        total,
        PaginationStyle::OffsetFull,
        PaginationMediaType::None,
    );
    Ok(response)
}

/// PATCH /ngsi-ld/v1/csourceRegistrations/{registration_id} -- Update a context source registration.
pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(registration_id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    let context = context.merge(body_context);
    service.update(&registration_id, payload, &context).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceRegistrations/{registration_id} -- Delete a context source registration.
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(registration_id): Path<String>) -> Result<Response, NgsiLdError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    service.delete(&registration_id).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
