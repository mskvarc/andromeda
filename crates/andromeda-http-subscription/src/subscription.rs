//! Handlers for the `/subscriptions` endpoints.

use crate::{
    error::SubscriptionResultExt,
    listing::ListingRequest,
    state::{SubscriptionTenantState, SubscriptionTenantStateOrCreate},
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    context::ContextUrl,
    context_target::{ExistingTargetedTenantState, ProvisionableTargetedTenantState},
    jsonld::JsonLd,
    pagination_policy::RequestPagination,
    prefer::PreferHeader,
    query::ValidatedQuery,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{headers::with_location, problem_response::ProblemResponse, rendered_response::RenderedResponse, single::NgsiLdResponse};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
use andromeda_ngsi_ld_render::subscription::SubscriptionView;
use andromeda_ngsi_ld_request::{empty::EmptyParams, subscription_listing::ListParams};
use andromeda_subscription::{subscription_context::SubscriptionContext, subscription_listing::SubscriptionListing};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/subscriptions -- Create a new subscription.
pub async fn create(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ProvisionableTargetedTenantState { services: state, target, .. }: SubscriptionTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd { body, context }: JsonLd<SubscriptionRequest>,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(&state.engine, context).await.with_subscription_instance(&uri)?;
    let request = body.admitted(&context)?;
    let service = &state.subscriptions;
    // Table 6.3.18-2: the created subscription must not forward a companion copy
    // back to any context source its request already passed through.
    let encountered_sources: Vec<ContextSourcePseudonym> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect();

    let id = service
        .create(
            &target,
            request,
            SubscriptionContext::Request {
                context: &context,
                host: &state.context_hosting.for_request(&api_root),
            },
            &encountered_sources,
        )
        .await
        .with_subscription_instance(&uri)?;

    Ok(with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/subscriptions/{id}")))
}

/// GET /ngsi-ld/v1/subscriptions/{subscription_id} -- Retrieve a subscription.
pub async fn get(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    TypedPath(subscription_id): TypedPath<Identifier>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    let options = params.list_options().with_subscription_instance(&uri)?;

    let subscription = service
        .get_for_response(&target, &subscription_id, &context)
        .await
        .with_subscription_instance(&uri)?;
    let view = SubscriptionView::of(&subscription, options.sys_attrs);

    // Clause 6.3.5: a GET is rendered under the context its `Link` header
    // names, or the default context. The subscription's own `jsonldContext`
    // governs its notifications (clause 5.8.1.4), not this response, and it
    // stays in the clause 5.2.12 document as a member.
    let ready = ResponseContextSource::new(&state.engine, &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_subscription_instance(&uri)?
        .with_body(&view);
    let rendered = RenderedResponse::resource(ready).with_subscription_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// GET /ngsi-ld/v1/subscriptions -- Query subscriptions.
pub async fn query(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    RequestPagination(pagination): RequestPagination,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    let listing = ListingRequest::of(pagination, &params, &uri)?;
    let SubscriptionListing {
        subscriptions,
        total,
        boundary,
    } = service
        .list_for_response(&target, &listing.window, &context)
        .await
        .with_subscription_instance(&uri)?;
    let returned = subscriptions.len();
    let views: Vec<SubscriptionView<'_>> = subscriptions
        .iter()
        .map(|subscription| SubscriptionView::of(subscription, listing.options.sys_attrs))
        .collect();

    let ready = ResponseContextSource::new(&state.engine, &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_subscription_instance(&uri)?
        .with_collection(&views);
    let rendered = RenderedResponse::resource(ready).with_subscription_instance(&uri)?;
    listing.respond(rendered, total, boundary, returned, &uri, accept.media_type())
}

/// PATCH /ngsi-ld/v1/subscriptions/{subscription_id} -- Update a subscription.
pub async fn update(
    // Clause 6.3.20: the operation defines no parameter, so any is an `InvalidRequest`.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Identifier>,
    JsonLd { body, context }: JsonLd<SubscriptionRequest>,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(&state.engine, context).await.with_subscription_instance(&uri)?;
    let request = body.admitted(&context)?;
    let service = &state.subscriptions;

    service
        .update(&target, &subscription_id, request, &context)
        .await
        .with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/subscriptions/{subscription_id} -- Delete a subscription.
pub async fn delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an `InvalidRequest`.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    service.delete(&target, &subscription_id).await.with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
