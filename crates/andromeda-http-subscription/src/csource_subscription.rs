//! Handlers for the `/csourceSubscriptions` endpoints.

use crate::{
    error::SubscriptionResultExt,
    listing::ListingRequest,
    state::{CsourceSubscriptionTenantState, CsourceSubscriptionTenantStateOrCreate},
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    context::ContextUrl,
    jsonld::JsonLdDocument,
    pagination_policy::RequestPagination,
    prefer::PreferHeader,
    query::ValidatedQuery,
    tenant_state::{ExistingTenantState, ProvisionableTenantState},
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{headers::with_location, problem_response::ProblemResponse, single::NgsiLdResponse};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_render::{response::ResponsePayload, subscription::SubscriptionRenderer};
use andromeda_ngsi_ld_request::{empty::EmptyParams, subscription_listing::ListParams};
use andromeda_subscription::{subscription_context::SubscriptionContext, subscription_listing::SubscriptionListing};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/csourceSubscriptions -- Create a context source subscription.
pub async fn create(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ProvisionableTenantState(state): CsourceSubscriptionTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    IncomingVia(incoming_via): IncomingVia,
    JsonLdDocument { document: payload, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    // Table 6.3.18-2: the created subscription must not forward a companion copy
    // back to any context source its request already passed through.
    let encountered_sources: Vec<ContextSourcePseudonym> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect();

    let id = service
        .create(
            &state.live_target,
            payload,
            SubscriptionContext::Request {
                context: &context,
                host: &state.context_hosting.for_request(&api_root),
            },
            &encountered_sources,
        )
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
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    TypedPath(subscription_id): TypedPath<Identifier>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let options = params.list_options().with_subscription_instance(&uri)?;

    let subscription = service
        .get_for_response(&state.live_target, &subscription_id, &context)
        .await
        .with_subscription_instance(&uri)?;

    let subscription = SubscriptionRenderer::new(subscription).with_sys_attrs(options.sys_attrs).render();

    let rendered = ResponseContextSource::new(&state.engine, &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_subscription_instance(&uri)?
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
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    RequestPagination(pagination): RequestPagination,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let listing = ListingRequest::of(pagination, &params, &uri)?;
    let SubscriptionListing {
        subscriptions,
        total,
        boundary,
    } = service
        .list_for_response(&state.live_target, &listing.window, &context)
        .await
        .with_subscription_instance(&uri)?;
    let returned = subscriptions.len();

    let subscriptions = subscriptions
        .into_iter()
        .map(|sub| SubscriptionRenderer::new(sub).with_sys_attrs(listing.options.sys_attrs).render())
        .collect::<Vec<_>>();

    let rendered = ResponseContextSource::new(&state.engine, &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_subscription_instance(&uri)?
        .with_payload(ResponsePayload::Collection(subscriptions))
        .render()
        .with_subscription_instance(&uri)?;
    listing.respond(rendered, total, boundary, returned, &uri, accept.media_type())
}

/// PATCH /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Update a context source subscription.
pub async fn update(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Identifier>,
    JsonLdDocument { document: payload, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;

    service
        .update(&state.live_target, &subscription_id, payload, &context)
        .await
        .with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Delete a context source subscription.
pub async fn delete(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    service.delete(&state.live_target, &subscription_id).await.with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
