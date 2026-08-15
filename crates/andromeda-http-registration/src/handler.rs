//! Handlers for the `/csourceRegistrations` endpoints.

use crate::{
    error::{RegistrationResultExt, internal_problem},
    state::{RegistrationTenantState, RegistrationTenantStateOrCreate},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    prefer::PreferHeader,
    query::ValidatedQuery,
    tenant_state::{ExistingTenantState, ProvisionableTenantState},
    typed_path::TypedPath,
};
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    headers::with_location,
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

pub(crate) async fn create(
    ProvisionableTenantState(state): RegistrationTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let identifier = state
        .registrations
        .create(payload, &context.merge(body_context))
        .await
        .with_registration_instance(&uri)?;
    Ok(with_location(
        StatusCode::CREATED.into_response(),
        format!("/ngsi-ld/v1/csourceRegistrations/{identifier}"),
    ))
}

pub(crate) async fn get(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    TypedPath(registration_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();
    let registration = state.registrations.get(&registration_id, &context).await.with_registration_instance(&uri)?;
    let payload = RegistrationRenderer::new(registration)
        .with_sys_attrs(params.parse_options().sys_attrs)
        .render();
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(payload))
        .render()
        .with_registration_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

pub(crate) async fn query(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();
    let limit = i64::from(params.effective_limit());
    let offset = i64::from(params.effective_offset());
    let (registrations, total) = state.registrations.query(&params, &context).await.with_registration_instance(&uri)?;
    let registrations = registrations
        .into_iter()
        .map(|registration| RegistrationRenderer::new(registration).with_sys_attrs(options.sys_attrs).render())
        .collect::<Vec<_>>();
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(registrations))
        .render()
        .with_registration_instance(&uri)?;
    let total_count =
        usize::try_from(total).map_err(|error| internal_problem(format!("registration count does not fit the response platform: {error}"), &uri))?;
    let pagination_total =
        i64::try_from(total).map_err(|error| internal_problem(format!("registration count does not fit offset pagination: {error}"), &uri))?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_total_count(total_count).into_response();
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

pub(crate) async fn update(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(registration_id): TypedPath<Uri>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    state
        .registrations
        .update(&registration_id, payload, &context.merge(body_context))
        .await
        .with_registration_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn delete(
    ExistingTenantState(state): RegistrationTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(registration_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    state.registrations.delete(&registration_id).await.with_registration_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
