//! HTTP handling for temporal query (clause 5.7.4).
//!
//! The operation has two HTTP bindings. Clause 6.18.3.2 carries the query in a
//! query string, clause 6.24.3.1 carries it in a clause 5.2.23 body, and clause
//! 6.24.1 says of the latter that "the behaviour of this clause mirrors the one
//! in clause 6.18.3.2". They therefore differ only in how the query reaches the
//! broker: once it has been decoded, [`run`] is the operation, and both
//! bindings hand it the same [`TemporalQueryRequest`].

use crate::{
    history_read::TemporalHistoryRead,
    join::apply_temporal_join,
    state::TemporalRouteServices,
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        Response,
        ResultInstanceExt,
        StatusCode,
        TemporalQueryParameters,
        TenantState,
        ValidatedQuery,
        query::{
            forwarding::{QueryScope, TemporalForwarding, forward_temporal_history},
            merged_history::merge_forwarded_history,
            merged_page::{MergedPageCut, cut_merged_page, defer_split_filters, hold_page_until_merge},
            page::QueriedPage,
            request_entity_map::{record_query_map, resolve_query_map},
            response::{TemporalCollectionMetadata, query_response, simplified_collection},
        },
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::pagination::PagePointers;
use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::{error::TemporalError, linked::candidates::TemporalLinkedCandidate, service::TemporalQueryWarning};
use axum::http::{HeaderMap, Uri};
use serde_json::Value;
use std::sync::Arc;

/// Asking the registered context sources for their part of the history.
mod forwarding;
/// Folding what those sources returned into the locally read history.
mod merged_history;
/// The page the query answers with, cut from the merged history.
mod merged_page;
/// The clause 6.3.10 element page the response points around.
mod page;
/// The `EntityMap` the query runs under.
mod request_entity_map;
/// Rendering the collection the query answers with.
mod response;

/// One "Query Temporal Evolution of Entities" as its binding decoded it.
///
/// Everything here is what the operation needs and nothing is specific to the
/// binding that supplied it: the query string binding of clause 6.18.3.2 fills
/// [`TemporalQueryRequest::params`] from the URL, the POST binding of clause
/// 6.24.3.1 fills it from the clause 5.2.23 body, and from there on the two are
/// the same request.
pub(crate) struct TemporalQueryRequest {
    /// The tenant-scoped capabilities the query runs against.
    pub(crate) services: Arc<TemporalRouteServices>,
    /// The context branch the query reads.
    pub(crate) target: ContextTarget,
    /// The representation the response was negotiated to (clause 6.3.4).
    pub(crate) accept: AcceptFormatNoGeo,
    /// The clause 6.3.x protocol headers: the `NGSILD-EntityMap` the request
    /// runs under, the `Via` chain it arrived with, and the extension headers
    /// that add to the query.
    pub(crate) protocol: NgsiLdProtocolHeaders,
    /// The URI the client addressed, which names the `instance` of every
    /// problem the operation reports and anchors its pagination links.
    pub(crate) uri: Uri,
    /// The request's headers, replayed onto every forwarded copy so a
    /// registration's `contextSourceInfo` can pass one of them through
    /// (clause 6.3.19).
    pub(crate) headers: HeaderMap,
    /// The JSON-LD context the query's terms expand under (clause 6.3.5).
    pub(crate) context: ContextSpec,
    /// The query itself, in the raw form the binding decoded it to.
    pub(crate) params: TemporalQueryParameters,
    /// Whether the binding's own request URI can carry the clause 6.3.10
    /// pointers to the neighbouring pages, which the query-string binding's
    /// can and the POST binding's cannot.
    pub(crate) pointers: PagePointers,
}

pub async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: TenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via: _,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ProblemResponse> {
    run(TemporalQueryRequest {
        services,
        target,
        accept,
        protocol,
        uri,
        headers,
        context: context.into_inner(),
        params,
        // Clause 6.18.3.2 carries the whole query in the request URI, so a
        // pointer built from it retrieves the next page on its own.
        pointers: PagePointers::OverRequestUri,
    })
    .await
}

/// Run "Query Temporal Evolution of Entities" (clause 5.7.4) over a decoded
/// request, whichever binding decoded it.
///
/// # Errors
///
/// The classified failure of resolving the Entity Map the query runs under, of
/// reading the local history, of merging what the context sources contributed,
/// of walking the clause 4.5.23 join, or of rendering the negotiated
/// representation; and the Table 6.4.3.2-3 `NotImplemented` answer when the one
/// source holding all of the data declined that representation.
pub(crate) async fn run(request: TemporalQueryRequest) -> Result<Response, ProblemResponse> {
    let TemporalQueryRequest {
        services: state,
        target,
        accept,
        protocol,
        uri,
        headers,
        context,
        mut params,
        pointers,
    } = request;
    let service = &state.temporal;
    protocol.extensions().apply_to_temporal_query(&mut params).with_instance(&uri)?;
    let entity_map_header = protocol.entity_map;
    let incoming_via = protocol.incoming_via;
    let key_values = params.is_simplified();

    let map_use = resolve_query_map(&state.entity_maps, &target, &mut params, entity_map_header.as_ref(), &uri).await?;

    if !target.permits_federation() {
        params.local = Some(true);
    }
    params.validate(true).with_instance(&uri)?;

    // Clause 6.3.10: the page the client asked for, read before the merge
    // widens the local query below, so the links point at the client's own
    // window rather than at the widened one.
    let page = QueriedPage::of(&params, pointers);

    let scope = QueryScope::of(&target, &params);
    let split = state.forwarding.split_entities(params.split_entities);
    let filtering = scope.splitting(split);

    // Forwarding reads the query as the client wrote it, so it runs before the
    // split-entities filters are held back from the local query below.
    let forwarded = forward_temporal_history(
        TemporalForwarding {
            forwarding: &state.forwarding,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
            split,
            map_use: &map_use,
        },
        scope,
        service.engine(),
        &target,
        &params,
        &context,
        &uri,
    )
    .await?;

    let deferred = defer_split_filters(&mut params, filtering);
    let merged_page_bounds = hold_page_until_merge(&mut params, forwarded.as_deref());

    let local_result = service.query(&target, &params, &context).await.with_instance(&uri)?;
    log_query_warnings(&local_result.warnings);
    let local_total = local_match_count(local_result.total_count).with_instance(&uri)?;
    let merged = merge_forwarded_history(local_result.entities, forwarded, filtering);

    let (mut entities, total_count) = cut_merged_page(
        service.engine(),
        &context,
        merged.entities,
        MergedPageCut {
            deferred,
            filtering,
            bounds: merged_page_bounds,
            local_total,
        },
        &params,
        &uri,
    )
    .await?;
    // Clause 6.3.10 pages the matching entities; a flat join appends the
    // entities they link to, which accompany the page rather than filling it.
    let returned = entities.len();

    // Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and an
    // absent `join` retrieve no linked entity at all, so neither reaches the
    // traversal. The join runs after forwarding and after the deferred
    // `splitEntities` filtering, so the roots it walks are the ones the client
    // will actually be shown.
    // Clause 6.3.13 counts the matches the query returned, which the linked
    // entities are not: they accompany the result set rather than belonging to
    // it. Counting before the flat join appends is what keeps `count` and the
    // 206 decision about the roots alone. The join also resolves before
    // EntityMap creation so a requested map can include the linked ids
    // (spec 5.5.14, 5.7.4.4).
    let read = TemporalHistoryRead {
        service: &state.temporal,
        target: &target,
        params: &params,
        context: &context,
    };
    let (warnings, linked_ids) = join_linked_entities(&read, &mut entities, &local_result.linked_candidates, merged.warnings, &uri).await?;

    let map_created = map_use.mints();
    let entity_map_uri = record_query_map(&state.entity_maps, &read, map_use, merged.contributions, linked_ids, &uri).await?;

    let values = if key_values { simplified_collection(entities, &uri)? } else { entities };

    // Table 6.18.3.2-2 and Table 6.24.3.1-1: 201 when an EntityMap has been
    // (re)created, 206 when the local history was cut short by the broker's own
    // lastN cap.
    let status = if local_result.any_truncated && local_result.range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else if map_created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    // Table 6.18.3.2-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is
    // what renders; clause 6.3.12's representation is what decides how clause
    // 4.3.6.8 reads an Attribute of the answer.
    query_response(
        accept,
        protocol.prefer,
        params.parse_format(),
        &context,
        values,
        TemporalCollectionMetadata {
            status,
            warnings,
            total_count,
            range: local_result.range,
            entity_map_uri,
            page,
            returned,
        },
        &uri,
    )
}

/// Record the non-fatal per-entity failures a temporal query collected.
fn log_query_warnings(warnings: &[TemporalQueryWarning]) {
    for warning in warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal query warning");
    }
}

/// The number of matches the local read reported, in the width the
/// `NGSILD-Results-Count` header carries.
///
/// # Errors
///
/// [`TemporalError::Internal`] when the store reported a negative count, which
/// counting rows cannot produce.
fn local_match_count(total: Option<i64>) -> Result<Option<usize>, TemporalError> {
    total
        .map(|count| usize::try_from(count).map_err(|_| TemporalError::internal("the local temporal read reported a negative match count")))
        .transpose()
}

/// Walk the join (clause 4.5.23) the query asked for, appending or inlining the
/// linked entities and reporting what the traversal could not reach.
///
/// `@none` and an absent `join` retrieve nothing, so neither reaches the
/// traversal and the merged warnings pass through untouched.
///
/// # Errors
///
/// The classified failure when the linked-entity query cannot be compiled or
/// the base entity store cannot be read.
async fn join_linked_entities(
    read: &TemporalHistoryRead<'_>,
    entities: &mut Vec<Value>,
    seed: &[TemporalLinkedCandidate],
    warnings: Vec<NgsiLdWarning>,
    instance: &Uri,
) -> Result<(Vec<NgsiLdWarning>, Vec<NgsiLdUri>), ProblemResponse> {
    let Some(join) = RetrievingJoin::from_mode(read.params.join) else {
        return Ok((warnings, Vec::new()));
    };
    let applied = apply_temporal_join(read, join, entities, seed, warnings).await.with_instance(instance)?;
    Ok((applied.warnings, applied.linked_ids))
}
