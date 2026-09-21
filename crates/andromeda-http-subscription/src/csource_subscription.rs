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
use andromeda_ngsi_ld_request::{csource_subscription::ListParams, empty::EmptyParams, pagination_cursor::decode_cursor};
use andromeda_subscription::keyset_listing::KeysetListing;
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
    // Clause 5.5.9.1: the opaque cursor is decoded here so a malformed one is a
    // BadRequestData rather than surfacing from the store; its exclusion with
    // offset was already held by `list_options`.
    let incoming = decode_cursor(params.cursor.as_deref()).with_subscription_instance(&uri)?;

    let limit = params.limit.unwrap_or(100);
    if limit == 0 && options.count == CountPolicy::Omit {
        return Err(public_problem("limit=0 requires the count option (clause 6.3.10)", &uri));
    }

    // Clause 5.5.9.2 vs 5.5.9.1: an explicit `offset` pages by offset, while its
    // absence walks the collection by opaque cursor over the identifier order --
    // the default, and what a followed `prev`/`next`/`last` continues.
    let (compacted, total, links) = if let Some(offset) = params.offset {
        let (subscriptions, total) = service
            .list_for_response(&state.live_target, Some(i64::from(limit)), Some(i64::from(offset)), &context)
            .await
            .with_subscription_instance(&uri)?;
        let links = PageLinks::Offset {
            offset: i64::from(offset),
            extent: PageExtent::Total(total),
        };
        (subscriptions, total, Some(links))
    } else {
        let KeysetListing {
            subscriptions,
            total,
            boundary,
        } = service
            .list_keyset_for_response(&state.live_target, incoming.as_ref(), i64::from(limit), &context)
            .await
            .with_subscription_instance(&uri)?;
        let links = keyset_page_links(incoming.as_ref(), boundary, subscriptions.len(), limit);
        (subscriptions, total, links)
    };

    let subscriptions = compacted
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
    // An empty cursor page has no boundary to walk from, so it carries no
    // pointers, matching the offset path's silence on an empty collection.
    if let Some(links) = links {
        append_pagination_links(&mut response, &uri, i64::from(limit), links, accept.media_type());
    }
    Ok(response)
}

/// Build the clause 5.5.9.1 cursor pointers for a keyset page from its boundary.
///
/// Returns `None` for an empty page, which has no boundary to page from and so
/// carries no pointers. Otherwise the neighbouring cursors are computed from the
/// first and last identifiers and whether the page filled the limit, then
/// encoded into the opaque tokens the `Link` header carries.
fn keyset_page_links(incoming: Option<&PageCursor>, boundary: Option<(Uri, Uri)>, returned: usize, limit: u32) -> Option<PageLinks> {
    let (first, last) = boundary?;
    let full = returned >= limit as usize;
    let neighbours = PageCursor::neighbours(incoming, first, last, full);
    Some(PageLinks::Cursor(CursorPointers {
        prev: neighbours.prev.map(|cursor| cursor.encode()),
        next: neighbours.next.map(|cursor| cursor.encode()),
        last: Some(neighbours.last.encode()),
    }))
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
