//! Handlers for the `/csourceSubscriptions` endpoints.

use crate::{
    error::{SubscriptionResultExt, internal_problem, public_problem},
    state::{CsourceSubscriptionTenantState, CsourceSubscriptionTenantStateOrCreate},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    prefer::PreferHeader,
    query::ValidatedQuery,
    tenant_state::{ExistingTenantState, ProvisionableTenantState},
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    headers::with_location,
    pagination::{PageExtent, PaginationStyle, append_pagination_links},
    problem_response::ProblemResponse,
    render::start_rendering_normalized,
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use andromeda_ngsi_ld_query::pagination::CountPolicy;
use andromeda_ngsi_ld_render::{response::ResponsePayload, subscription::SubscriptionRenderer};
use andromeda_ngsi_ld_request::{csource_subscription::ListParams, empty::EmptyParams};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// POST /ngsi-ld/v1/csourceSubscriptions -- Create a context source subscription.
pub async fn create(
    ProvisionableTenantState(state): CsourceSubscriptionTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context = context.merge(body_context);
    // Table 6.3.18-2: the created subscription must not forward a companion copy
    // back to any context source its request already passed through.
    let encountered_sources: Vec<ContextSourcePseudonym> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect();

    let id = service
        .create(&state.live_target, payload, &context, None, &encountered_sources)
        .await
        .with_subscription_instance(&uri)?;

    // Clause 6.12.3.1: the Location is the relative path of the created
    // resource, not the bare identifier.
    Ok(with_location(
        StatusCode::CREATED.into_response(),
        format!("/ngsi-ld/v1/csourceSubscriptions/{id}"),
    ))
}

/// GET /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Retrieve a context source subscription.
pub async fn get(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    TypedPath(subscription_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.list_options().with_subscription_instance(&uri)?;

    let subscription = service
        .get_for_response(&state.live_target, &subscription_id, &context)
        .await
        .with_subscription_instance(&uri)?;

    let subscription = SubscriptionRenderer::new(subscription).with_sys_attrs(options.sys_attrs).render();

    let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(subscription))
        .render()
        .with_subscription_instance(&uri)?;
    let response = NgsiLdResponse::ok(rendered).into_response();
    Ok(response)
}

/// GET /ngsi-ld/v1/csourceSubscriptions -- Query context source subscriptions.
pub async fn query(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.list_options().with_subscription_instance(&uri)?;

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
        .list_for_response(&state.live_target, Some(i64::from(limit)), Some(i64::from(offset)), &context)
        .await
        .with_subscription_instance(&uri)?;

    let subscriptions = subscriptions
        .into_iter()
        .map(|sub| SubscriptionRenderer::new(sub).with_sys_attrs(options.sys_attrs).render())
        .collect::<Vec<_>>();

    let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(subscriptions))
        .render()
        .with_subscription_instance(&uri)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered);

    if options.count == CountPolicy::Emit {
        // The store counts rows, so the total is never negative; a conversion
        // that fails here is an internal inconsistency, not client input.
        let total_count = usize::try_from(total).map_err(|error| internal_problem(&error, &uri))?;
        response = response.with_total_count(total_count);
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
    ExistingTenantState(state): CsourceSubscriptionTenantState,
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
        .update(&state.live_target, &subscription_id, payload, &context, None)
        .await
        .with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Delete a context source subscription.
pub async fn delete(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    service.delete(&state.live_target, &subscription_id).await.with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
