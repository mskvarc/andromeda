use crate::{
    api::{
        handler::parse::{check_json_content_type, extract_body_context, is_json_content_type, parse_list_options},
        tenant::TenantState,
    },
    error::api::ApiError,
    http::{
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery},
        parameter::{EmptyParams, csource_subscription::ListParams},
        response::{
            collection::NgsiLdCollectionResponse,
            pagination::{PaginationMediaType, PaginationStyle, append_pagination_links},
            single::NgsiLdResponse,
        },
    },
    jsonld::ContextSpec,
    renderer::{response::ResponseRenderer, subscription::render_csource_subscription},
};
use axum::{
    body::Bytes,
    extract::{OriginalUri, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

pub async fn create(
    crate::api::tenant::TenantStateOrCreate(state): crate::api::tenant::TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    context: ContextUrl,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !is_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::from_error(crate::error::AndromedaError::invalid_request(format!("Invalid JSON: {}", e)), uri.clone()))?;
    let service = &state.csr_subscriptions;
    let context = if let Some(body_ctx) = extract_body_context(&payload) {
        context.into_inner().with_body_context(Some(body_ctx))
    } else {
        context.into_inner()
    };

    let id = service.create(payload, &context, None).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", id)]).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.csr_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let mut subscription = service
        .get_for_response(&subscription_id, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    render_csource_subscription(&mut subscription, options.sys_attrs);

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(subscription);
    let response = NgsiLdResponse::ok(rendered).into_response();
    Ok(response)
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.csr_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let limit = params.limit.unwrap_or(100);
    if limit <= 0 {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("limit must be greater than 0"),
            uri.clone(),
        ));
    }
    if let Some(page) = params.page
        && page <= 0
    {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("page must be greater than 0"),
            uri.clone(),
        ));
    }
    let offset = if let Some(page) = params.page {
        (page - 1) * limit
    } else {
        let offset = params.offset.unwrap_or(0);
        if offset < 0 {
            return Err(ApiError::from_error(
                crate::error::AndromedaError::bad_request_data("offset must be greater than or equal to 0"),
                uri.clone(),
            ));
        }
        offset
    };

    let (mut subscriptions, total) = service
        .list_for_response(Some(limit), Some(offset), &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    for sub in &mut subscriptions {
        render_csource_subscription(sub, options.sys_attrs);
    }

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_collection(subscriptions);
    let mut response = NgsiLdCollectionResponse::ok(rendered);

    if options.count {
        response = response.with_total_count(total as usize);
    }

    let mut response = response.into_response();
    append_pagination_links(
        &mut response,
        &uri,
        limit,
        offset,
        total,
        PaginationStyle::PagePrevNext,
        PaginationMediaType::JsonLd,
    );
    Ok(response)
}

pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if check_json_content_type(&headers).is_err() {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::from_error(crate::error::AndromedaError::bad_request_data(format!("Invalid JSON: {}", e)), uri.clone()))?;
    let service = &state.csr_subscriptions;
    let context = if let Some(body_ctx) = extract_body_context(&payload) {
        context.into_inner().with_body_context(Some(body_ctx))
    } else {
        context.into_inner()
    };

    service
        .update(&subscription_id, payload, &context, None)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(subscription_id): Path<String>) -> Result<Response, ApiError> {
    let service = &state.csr_subscriptions;
    service.delete(&subscription_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
