use crate::{
    api::tenant::TenantState,
    error::api::ApiError,
    http::{
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, prefer::PreferHeader, query::ValidatedQuery},
        response::{
            collection::NgsiLdCollectionResponse,
            pagination::{PaginationMediaType, PaginationStyle, append_pagination_links},
            single::NgsiLdResponse,
        },
    },
    jsonld::ContextSpec,
    query::csr::CsrQueryParameters,
    renderer::{registration::render_registration, response::ResponseRenderer},
};
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use url::Url;
use urn::Urn;

fn parse_csr_id(id: &str, uri: &axum::http::Uri) -> Result<(), ApiError> {
    if id.starts_with("urn:") {
        id.parse::<Urn>().map_err(|_| {
            ApiError::from_error(
                crate::error::AndromedaError::bad_request_data("Invalid registration id: not a valid URI"),
                uri.clone(),
            )
        })?;
    } else {
        Url::parse(id).map_err(|_| {
            ApiError::from_error(
                crate::error::AndromedaError::bad_request_data("Invalid registration id: not a valid URI"),
                uri.clone(),
            )
        })?;
    }
    Ok(())
}

pub async fn create(
    crate::api::tenant::TenantStateOrCreate(state): crate::api::tenant::TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
    let service = &state.csr;
    let context = context.merge(body_context);

    let id = service.create(payload, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/csourceRegistrations/{}", id))]).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(registration_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ApiError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let mut csr = service.get(&registration_id, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    render_registration(&mut csr, options.sys_attrs);

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(csr);
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<CsrQueryParameters>,
) -> Result<Response, ApiError> {
    use crate::query::params::SharedQueryParams;

    let service = &state.csr;
    let context: ContextSpec = context.into_inner();
    let options = params.parse_options();

    let limit = params.effective_limit() as i64;
    let offset = params.effective_offset() as i64;

    let (mut csrs, total) = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    for csr in &mut csrs {
        render_registration(csr, options.sys_attrs);
    }

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_collection(csrs);
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

pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(registration_id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    let context = context.merge(body_context);
    service
        .update(&registration_id, payload, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(registration_id): Path<String>) -> Result<Response, ApiError> {
    parse_csr_id(&registration_id, &uri)?;

    let service = &state.csr;
    service.delete(&registration_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
