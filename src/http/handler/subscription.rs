//! Handlers for the `/subscriptions` endpoints.

use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, prefer::PreferHeader, query::ValidatedQuery},
        response::{
            collection::NgsiLdCollectionResponse,
            pagination::{PaginationMediaType, PaginationStyle, append_pagination_links},
            render::start_rendering,
            single::NgsiLdResponse,
        },
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_query::pagination::CountPolicy;
use andromeda_ngsi_ld_render::{
    response::ResponsePayload,
    subscription::{JsonldContextStripRenderer, SubscriptionFlavor, SubscriptionRenderer},
};
use andromeda_ngsi_ld_request::{empty::EmptyParams, subscription::ListParams};
use axum::{
    extract::{OriginalUri, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use iri_rs::IriRefBuf;
use serde_json::Value;

/// POST /ngsi-ld/v1/subscriptions -- Create a new subscription.
pub async fn create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.subscriptions;
    let base_url = headers.get("host").and_then(|v| v.to_str().ok()).map(|h| format!("http://{}", h));
    let context = context.merge(body_context);

    let id = service.create(payload, &context, base_url.as_deref()).await.with_instance(&uri)?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/subscriptions/{}", id))]).into_response())
}

/// GET /ngsi-ld/v1/subscriptions/{subscription_id} -- Retrieve a subscription.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.options.unwrap_or_default();

    let subscription = service.get_for_response(&subscription_id, &context).await.with_instance(&uri)?;

    let subscription = SubscriptionRenderer::new(subscription, SubscriptionFlavor::Local)
        .with_sys_attrs(options.sys_attrs)
        .render();

    // Extract jsonldContext from body and use it for the response context
    let strip = JsonldContextStripRenderer::new(subscription).render();
    // The extracted jsonldContext is a context URL when it parses as an IRI
    // reference; a body-embedded inline context (an object or array) is not one
    // and leaves the request's own context in force.
    let response_context = match strip
        .extracted
        .as_ref()
        .and_then(|v| v.as_str())
        .and_then(|url| IriRefBuf::new(url.to_string()).ok())
    {
        Some(url) => ContextSpec::from_links(vec![url]),
        None => context,
    };
    let subscription = strip.payload;

    let rendered = start_rendering(accept.media_type(), &response_context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(subscription))
        .render()?;
    let response = NgsiLdResponse::ok(rendered).into_response();
    Ok(response)
}

/// GET /ngsi-ld/v1/subscriptions -- Query subscriptions.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.options.unwrap_or_default();

    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);
    if limit == 0 && options.count == CountPolicy::Omit {
        return Err(AndromedaError::entity_validation("limit=0 requires the count option (clause 6.3.10)")
            .with_instance(&uri)
            .into());
    }

    let (subscriptions, total) = service
        .list_for_response(Some(i64::from(limit)), Some(i64::from(offset)), &context)
        .await
        .with_instance(&uri)?;

    let subscriptions = subscriptions
        .into_iter()
        .map(|sub| {
            let sub = SubscriptionRenderer::new(sub, SubscriptionFlavor::Local)
                .with_sys_attrs(options.sys_attrs)
                .render();
            // Strip jsonldContext from each subscription body
            JsonldContextStripRenderer::new(sub).render().payload
        })
        .collect::<Vec<_>>();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Collection(subscriptions))
        .render()?;
    let mut response = NgsiLdCollectionResponse::ok(rendered);

    if options.count == CountPolicy::Emit {
        response = response.with_total_count(total as usize);
    }

    let mut response = response.into_response();
    append_pagination_links(
        &mut response,
        &uri,
        i64::from(limit),
        i64::from(offset),
        total,
        PaginationStyle::OffsetPrevNext,
        PaginationMediaType::JsonLd,
    );
    Ok(response)
}

/// PATCH /ngsi-ld/v1/subscriptions/{subscription_id} -- Update a subscription.
pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    Path(subscription_id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.subscriptions;
    let base_url = headers.get("host").and_then(|v| v.to_str().ok()).map(|h| format!("http://{}", h));
    let context = context.merge(body_context);

    service
        .update(&subscription_id, payload, &context, base_url.as_deref())
        .await
        .with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/subscriptions/{subscription_id} -- Delete a subscription.
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(subscription_id): Path<String>) -> Result<Response, NgsiLdError> {
    let service = &state.subscriptions;
    service.delete(&subscription_id).await.with_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
