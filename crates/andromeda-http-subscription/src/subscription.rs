//! Handlers for the `/subscriptions` endpoints.

use crate::{
    error::{SubscriptionResultExt, internal_problem, public_problem},
    state::{SubscriptionTenantState, SubscriptionTenantStateOrCreate},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    context_target::{ExistingTargetedTenantState, ProvisionableTargetedTenantState},
    jsonld::JsonLdDocument,
    origin::RequestOrigin,
    prefer::PreferHeader,
    query::ValidatedQuery,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_headers::via::Via;
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    headers::with_location,
    internal_failure_summary::InternalFailureSummary,
    pagination::{CursorPointers, PageExtent, PageLinks, append_pagination_links},
    problem_response::ProblemResponse,
    render::start_rendering_normalized,
    single::NgsiLdResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use andromeda_ngsi_ld_query::{cursor::PageCursor, pagination::CountPolicy};
use andromeda_ngsi_ld_render::{response::ResponsePayload, subscription::SubscriptionRenderer};
use andromeda_ngsi_ld_request::{empty::EmptyParams, pagination_cursor::decode_cursor, subscription::ListParams};
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/subscriptions -- Create a new subscription.
pub async fn create(
    ProvisionableTargetedTenantState { services: state, target, .. }: SubscriptionTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    RequestOrigin(base_url): RequestOrigin,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLdDocument {
        document: payload,
        context: body_context,
    }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    let context = context.merge(body_context);
    // Table 6.3.18-2: the created subscription must not forward a companion copy
    // back to any context source its request already passed through.
    let encountered_sources: Vec<ContextSourcePseudonym> = incoming_via.iter().flat_map(Via::pseudonyms).cloned().collect();

    let id = service
        .create(&target, payload, &context, base_url.as_ref(), &encountered_sources)
        .await
        .with_subscription_instance(&uri)?;

    Ok(with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/subscriptions/{id}")))
}

/// GET /ngsi-ld/v1/subscriptions/{subscription_id} -- Retrieve a subscription.
pub async fn get(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    TypedPath(subscription_id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    let context: ContextSpec = context.into_inner();
    let options = params.list_options().with_subscription_instance(&uri)?;

    let subscription = service
        .get_for_response(&target, &subscription_id, &context)
        .await
        .with_subscription_instance(&uri)?;

    let subscription = SubscriptionRenderer::new(subscription).with_sys_attrs(options.sys_attrs).render();

    // Clause 6.3.5: a GET is rendered under the context its `Link` header
    // names, or the default context. The subscription's own `jsonldContext`
    // governs its notifications (clause 5.8.1.4), not this response, and it
    // stays in the clause 5.2.12 document as a member.
    let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(subscription))
        .render()
        .with_subscription_instance(&uri)?;
    let response = NgsiLdResponse::ok(rendered).into_response();
    Ok(response)
}

/// GET /ngsi-ld/v1/subscriptions -- Query subscriptions.
pub async fn query(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    let context: ContextSpec = context.into_inner();
    // `list_options` also holds the clause 5.5.9 cursor/offset mutual exclusion.
    let options = params.list_options().with_subscription_instance(&uri)?;
    let incoming = decode_cursor(params.cursor.as_deref()).with_subscription_instance(&uri)?;

    let limit = params.limit.unwrap_or(100);
    if limit == 0 && options.count == CountPolicy::Omit {
        return Err(public_problem("limit=0 requires the count option (clause 6.3.10)", &uri));
    }

    // Clause 5.5.9.1: the subscription listing is locally scoped and ordered by
    // its identifier, so by default it walks by opaque cursor; an explicit
    // `offset` keeps the clause 5.5.9.2 offset listing.
    let (subscriptions, links, total) = if let Some(offset) = params.offset {
        let (subscriptions, total) = service
            .list_for_response(&target, Some(i64::from(limit)), Some(i64::from(offset)), &context)
            .await
            .with_subscription_instance(&uri)?;
        let links = Some(PageLinks::Offset {
            offset: i64::from(offset),
            extent: PageExtent::Total(total),
        });
        (subscriptions, links, total)
    } else {
        let listing = service
            .list_keyset_for_response(&target, incoming.as_ref(), i64::from(limit), &context)
            .await
            .with_subscription_instance(&uri)?;
        let links = cursor_links(incoming.as_ref(), listing.boundary, listing.subscriptions.len(), limit);
        (listing.subscriptions, links, listing.total)
    };

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
        let total_count = usize::try_from(total).map_err(|error| internal_problem(&error, InternalFailureSummary::new("internal subscription error"), &uri))?;
        response = response.with_total_count(total_count);
    }

    let mut response = response.into_response();
    if let Some(links) = links {
        append_pagination_links(&mut response, &uri, i64::from(limit), links, accept.media_type());
    }
    Ok(response)
}

/// The opaque cursor pointers a keyset subscription page offers (clause
/// 5.5.9.1), or `None` when the page is empty and points at nothing.
fn cursor_links(incoming: Option<&PageCursor>, boundary: Option<(Uri, Uri)>, returned: usize, limit: u32) -> Option<PageLinks> {
    let (first, last) = boundary?;
    let full = returned == limit as usize;
    let neighbours = PageCursor::neighbours(incoming, first, last, full);
    Some(PageLinks::Cursor(CursorPointers::from(neighbours)))
}

/// PATCH /ngsi-ld/v1/subscriptions/{subscription_id} -- Update a subscription.
pub async fn update(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    RequestOrigin(base_url): RequestOrigin,
    context: ContextUrl,
    TypedPath(subscription_id): TypedPath<Uri>,
    JsonLdDocument {
        document: payload,
        context: body_context,
    }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    let context = context.merge(body_context);

    service
        .update(&target, &subscription_id, payload, &context, base_url.as_ref())
        .await
        .with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/subscriptions/{subscription_id} -- Delete a subscription.
pub async fn delete(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let service = &state.subscriptions;
    service.delete(&target, &subscription_id).await.with_subscription_instance(&uri)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
