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
    jsonld::JsonLd,
    pagination_policy::RequestPagination,
    path_segment::SubscriptionId,
    prefer::PreferHeader,
    query::ValidatedQuery,
    resource_location::{ResourceCollection, ResourceLocator},
    tenant_state::{ExistingTenantState, ProvisionableTenantState},
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{problem_response::ProblemResponse, rendered_response::RenderedResponse, single::NgsiLdResponse};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
use andromeda_ngsi_ld_render::subscription::SubscriptionView;
use andromeda_ngsi_ld_request::{empty::EmptyParams, subscription_listing::ListParams, sys_attrs_parameters::SysAttrsParams};
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
    locator: ResourceLocator,
    JsonLd { body, context }: JsonLd<SubscriptionRequest>,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(&state.engine, context).await.with_subscription_instance(&uri)?;
    let request = body.admitted(&context).await?;
    let service = &state.csource_subscriptions;
    // Table 6.3.18-2: the created subscription must not forward a companion copy
    // back to any context source its request already passed through.
    let encountered_sources: Vec<ContextSourcePseudonym> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect();

    let id = service
        .create(
            &state.live_target,
            request,
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
    Ok(locator.locate(StatusCode::CREATED.into_response(), ResourceCollection::ContextSourceSubscriptions, &id))
}

/// GET /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Retrieve a context source subscription.
pub async fn get(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    TypedPath(SubscriptionId(subscription_id)): TypedPath<SubscriptionId>,
    ValidatedQuery(params): ValidatedQuery<SysAttrsParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let options = params.parse_options().with_subscription_instance(&uri)?;

    let subscription = service
        .get_for_response(&state.live_target, &subscription_id, &context)
        .await
        .with_subscription_instance(&uri)?;
    let view = SubscriptionView::of(&subscription, options.sys_attrs);

    let ready = ResponseContextSource::new(&state.engine, &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .with_subscription_instance(&uri)?
        .with_body(&view);
    let rendered = RenderedResponse::resource(ready).with_subscription_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// GET /ngsi-ld/v1/csourceSubscriptions -- Query context source subscriptions.
pub async fn query(
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
    RequestPagination {
        policy: pagination,
        context: page_context,
    }: RequestPagination,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    let listing = ListingRequest::of(pagination, page_context, &params, &uri)?;
    let SubscriptionListing {
        subscriptions,
        total,
        boundary,
        reach,
    } = service
        .list_for_response(&state.live_target, &listing.window, &context)
        .await
        .with_subscription_instance(&uri)?;
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
    listing.respond(rendered, total, boundary, reach, &uri, accept.media_type())
}

/// PATCH /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Update a context source subscription.
pub async fn update(
    // Clause 6.3.20: the operation defines no parameter, so any is an `InvalidRequest`.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(SubscriptionId(subscription_id)): TypedPath<SubscriptionId>,
    JsonLd { body, context }: JsonLd<SubscriptionRequest>,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(&state.engine, context).await.with_subscription_instance(&uri)?;
    let request = body.admitted(&context).await?;
    let service = &state.csource_subscriptions;

    service
        .update(&state.live_target, &subscription_id, request, &context)
        .await
        .with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/csourceSubscriptions/{subscription_id} -- Delete a context source subscription.
pub async fn delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an `InvalidRequest`.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    ExistingTenantState(state): CsourceSubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(SubscriptionId(subscription_id)): TypedPath<SubscriptionId>,
) -> Result<Response, ProblemResponse> {
    let service = &state.csource_subscriptions;
    service.delete(&state.live_target, &subscription_id).await.with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
