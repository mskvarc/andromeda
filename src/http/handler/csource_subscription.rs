//! Handlers for the `/csourceSubscriptions` endpoints.

use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    core::error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, prefer::PreferHeader, query::ValidatedQuery},
        response::{
            collection::NgsiLdCollectionResponse,
            pagination::{PaginationMediaType, PaginationStyle, append_pagination_links},
            render::start_rendering,
            single::NgsiLdResponse,
        },
        validation::parse_list_options,
    },
    jsonld::ContextSpec,
};
use andromeda_ngsi_ld_render::{
    response::ResponsePayload,
    subscription::{SubscriptionFlavor, SubscriptionRenderer},
};
use andromeda_ngsi_ld_request::{csource_subscription::ListParams, empty::EmptyParams};
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/csourceSubscriptions -- Create a context source subscription.
pub async fn create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.csr_subscriptions;
    let context = context.merge(body_context);

    let id = service.create(payload, &context, None).await.with_instance(&uri)?;

    Ok((StatusCode::CREATED, [("Location", id)]).into_response())
}

/// GET /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Retrieve a context source subscription.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.csr_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let subscription = service.get_for_response(&subscription_id, &context).await.with_instance(&uri)?;

    let subscription = SubscriptionRenderer::new(subscription, SubscriptionFlavor::ContextSource)
        .with_sys_attrs(options.sys_attrs)
        .render();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(subscription))
        .render()?;
    let response = NgsiLdResponse::ok(rendered).into_response();
    Ok(response)
}

/// GET /ngsi-ld/v1/csourceSubscriptions -- Query context source subscriptions.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.csr_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = parse_list_options(params.options.as_deref(), &uri)?;

    let limit = params.limit.unwrap_or(100);
    if limit <= 0 {
        return Err(NgsiLdError::from(
            AndromedaError::query_validation("limit must be greater than 0").with_instance(&uri),
        ));
    }
    if let Some(page) = params.page
        && page <= 0
    {
        return Err(NgsiLdError::from(
            AndromedaError::query_validation("page must be greater than 0").with_instance(&uri),
        ));
    }
    let offset = if let Some(page) = params.page {
        (page - 1) * limit
    } else {
        let offset = params.offset.unwrap_or(0);
        if offset < 0 {
            return Err(NgsiLdError::from(
                AndromedaError::query_validation("offset must be greater than or equal to 0").with_instance(&uri),
            ));
        }
        offset
    };

    let (subscriptions, total) = service.list_for_response(Some(limit), Some(offset), &context).await.with_instance(&uri)?;

    let subscriptions = subscriptions
        .into_iter()
        .map(|sub| {
            SubscriptionRenderer::new(sub, SubscriptionFlavor::ContextSource)
                .with_sys_attrs(options.sys_attrs)
                .render()
        })
        .collect::<Vec<_>>();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Collection(subscriptions))
        .render()?;
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

/// PATCH /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Update a context source subscription.
pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.csr_subscriptions;
    let context = context.merge(body_context);

    service.update(&subscription_id, payload, &context, None).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Delete a context source subscription.
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(subscription_id): Path<String>) -> Result<Response, NgsiLdError> {
    let service = &state.csr_subscriptions;
    service.delete(&subscription_id).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
