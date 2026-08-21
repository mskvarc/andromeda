//! Handlers for the `/csourceSubscriptions` endpoints.

use crate::{
    error::{SubscriptionResultExt, public_problem},
    state::{SubscriptionTenantState, SubscriptionTenantStateOrCreate},
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
    pagination::{PageExtent, PaginationStyle, append_pagination_links},
    problem_response::ProblemResponse,
    render::start_rendering,
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::pagination::CountPolicy;
use andromeda_ngsi_ld_render::{
    response::ResponsePayload,
    subscription::{SubscriptionFlavor, SubscriptionRenderer},
};
use andromeda_ngsi_ld_request::{csource_subscription::ListParams, empty::EmptyParams};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/csourceSubscriptions -- Create a context source subscription.
pub async fn create(
    ProvisionableTenantState(state): SubscriptionTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context = context.merge(body_context);

    let id = service.create(payload, &context, None).await.with_subscription_instance(&uri)?;

    // Clause 6.12.3.1: the Location is the relative path of the created
    // resource, not the bare identifier.
    Ok(with_location(
        StatusCode::CREATED.into_response(),
        format!("/ngsi-ld/v1/csourceSubscriptions/{id}"),
    ))
}

/// GET /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Retrieve a context source subscription.
pub async fn get(
    ExistingTenantState(state): SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    TypedPath(subscription_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.options.unwrap_or_default();

    let subscription = service.get_for_response(&subscription_id, &context).await.with_subscription_instance(&uri)?;

    let subscription = SubscriptionRenderer::new(subscription, SubscriptionFlavor::ContextSource)
        .with_sys_attrs(options.sys_attrs)
        .render();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(subscription))
        .render()
        .with_subscription_instance(&uri)?;
    let response = NgsiLdResponse::ok(rendered).into_response();
    Ok(response)
}

/// GET /ngsi-ld/v1/csourceSubscriptions -- Query context source subscriptions.
pub async fn query(
    ExistingTenantState(state): SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.options.unwrap_or_default();

    let (limit, offset) = match (params.limit, params.offset) {
        (Some(limit), Some(offset)) => (limit, offset),
        (Some(limit), None) => (limit, 0),
        (None, Some(offset)) => (100, offset),
        (None, None) => (100, 0),
    };
    if limit == 0 && options.count == CountPolicy::Omit {
        return Err(public_problem("limit=0 requires the count option (clause 6.3.10)", &uri));
    }

    let (subscriptions, total) = service
        .list_for_response(Some(i64::from(limit)), Some(i64::from(offset)), &context)
        .await
        .with_subscription_instance(&uri)?;

    let subscriptions = subscriptions
        .into_iter()
        .map(|sub| {
            SubscriptionRenderer::new(sub, SubscriptionFlavor::ContextSource)
                .with_sys_attrs(options.sys_attrs)
                .render()
        })
        .collect::<Vec<_>>();

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(subscriptions))
        .render()
        .with_subscription_instance(&uri)?;
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
        PageExtent::Total(total),
        PaginationStyle::PagePrevNext,
        accept.media_type(),
    );
    Ok(response)
}

/// PATCH /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Update a context source subscription.
pub async fn update(
    ExistingTenantState(state): SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(subscription_id): TypedPath<Uri>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context = context.merge(body_context);

    service
        .update(&subscription_id, payload, &context, None)
        .await
        .with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Delete a context source subscription.
pub async fn delete(
    ExistingTenantState(state): SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    service.delete(&subscription_id).await.with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
