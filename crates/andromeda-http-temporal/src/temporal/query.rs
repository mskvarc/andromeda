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
    request_context_admission::admit_request_context,
    temporal::{
        ProblemResponse,
        Response,
        ResultInstanceExt,
        StatusCode,
        query::{
            admission::{AdmittedQuery, admit_request},
            complete_history::HistoryCompleteness,
            forwarding::{QueryScope, TemporalForwarding, forward_temporal_history},
            merged_history::merge_forwarded_history,
            merged_page::{MergedPageCut, cut_merged_page, defer_split_filters, hold_page_until_merge},
            page::QueriedPage,
            request::TemporalQueryRequest,
            request_entity_map::record_query_map,
            response::{TemporalCollectionMetadata, query_response, simplified_collection},
        },
    },
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_request::{join_traversal::RetrievingJoin, pagination_cursor::decode_cursor};
use andromeda_temporal::{
    error::TemporalError,
    linked::candidates::TemporalLinkedCandidate,
    service::{TemporalQueryWarning, TemporalRange},
    temporal_invariant::TemporalInvariant,
};
use axum::http::Uri;
use serde_json::Value;

/// Admitting a decoded query before it runs.
mod admission;
/// How far the merged history is complete (clause 6.3.10).
mod complete_history;
/// Asking the registered context sources for their part of the history.
mod forwarding;
/// Folding what those sources returned into the locally read history.
mod merged_history;
/// The page the query answers with, cut from the merged history.
mod merged_page;
/// The clause 6.3.10 element page the response points around.
mod page;
/// The query-string binding, and the request both bindings hand the operation.
pub(crate) mod request;
/// The `EntityMap` the query runs under.
mod request_entity_map;
/// Rendering the collection the query answers with.
mod response;

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
        api_root,
        headers,
        context,
        mut params,
        pointers,
    } = request;
    let service = &state.temporal;
    // Clause 5.5.7: a request `@context` declaring a JSON-LD Scoped Context is
    // refused on the temporal query path -- GET and POST alike -- before it is
    // forwarded or expanded against.
    admit_request_context(service.engine(), &context, &uri).await?;
    let AdmittedQuery { prefer, incoming_via, map_use } = admit_request(&state, &target, &context, protocol, &mut params, &uri).await?;
    let key_values = params.is_simplified();

    // Clause 5.5.9.1: decode the opaque pagination cursor before the page is
    // planned, so a malformed one is a BadRequestData here rather than surfacing
    // from the store. Its mutual exclusion with offset was already held by
    // validation.
    let incoming_cursor = decode_cursor(params.cursor.as_deref()).with_instance(&uri)?;
    // Clause 6.3.10: the page the client asked for, read before the merge
    // widens the local query below, so the links point at the client's own
    // window rather than at the widened one.
    let page = QueriedPage::of(&params, pointers, incoming_cursor);

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
    let merged_page_window = hold_page_until_merge(&mut params, forwarded.as_ref(), &page);

    let local_result = service.query(&target, &params, &context).await.with_instance(&uri)?;
    log_query_warnings(&local_result.warnings);
    let local_total = local_match_count(local_result.total_count).with_instance(&uri)?;
    let completeness = HistoryCompleteness::of(&params, local_result.range, forwarded.as_ref()).with_instance(&uri)?;
    let mut merged = merge_forwarded_history(local_result.entities, forwarded, filtering, &uri)?;
    let timeproperty = params.timeproperty_or_default();
    completeness.trim(&mut merged.entities, timeproperty);

    let (mut entities, total_count) = cut_merged_page(
        service,
        &context,
        merged.entities,
        MergedPageCut {
            deferred,
            filtering,
            window: merged_page_window,
            local_total,
        },
        &params,
        &uri,
    )
    .await?;
    // Clause 6.3.10: the `Content-Range` describes the history the page's own
    // entities carry, read before a flat join appends the linked ones.
    let range = completeness.range(&entities, timeproperty);
    // Clause 6.3.10 pages the matching entities; a flat join appends the
    // entities they link to, which accompany the page rather than filling it.
    let returned = entities.len();
    // Clause 5.5.9.1: the boundary a cursor pointer walks from is the first and
    // last identifiers of the matching page, captured here before the flat join
    // appends the linked entities the page must not point past.
    let page_boundary = element_boundary(&entities);

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

    let status = response_status(range.as_ref(), map_created);
    // Table 6.18.3.2-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is
    // what renders; clause 6.3.12's representation is what decides how clause
    // 4.3.6.8 reads an Attribute of the answer. Table 6.24.3.1-1 prints no 203
    // row for the POST form, but clause 6.24.3.1 gives it the clause 5.7.4
    // behaviour of the GET and clause 6.3.6 ties the echo to any response
    // carrying a payload body, so both bindings answer the same way.
    let rendering = ResponseContextSource::new(service.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_temporal(accept.media_type(), prefer.rendering_preferences(), params.selected_format())
        .await
        .with_instance(&uri)?;
    query_response(
        accept,
        rendering,
        values,
        TemporalCollectionMetadata {
            status,
            warnings,
            total_count,
            range,
            entity_map_uri,
            page,
            returned,
            boundary: page_boundary,
        },
        &uri,
    )
}

/// The status a temporal query answers with (Table 6.18.3.2-2 and Table
/// 6.24.3.1-1): 206 when the history -- the local read's or a context
/// source's -- was cut short at `range`, else 201 when an `EntityMap` has been
/// (re)created, else 200.
const fn response_status(range: Option<&TemporalRange>, map_created: bool) -> StatusCode {
    if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else if map_created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    }
}

/// The first and last entity identifiers of the matching element page, from
/// which a clause 5.5.9.1 cursor pointer is computed.
///
/// Absent when the page is empty or when an entity carries no readable `id`, in
/// which case the cursor pointers are not offered and the offset path stands.
fn element_boundary(entities: &[Value]) -> Option<(Identifier, Identifier)> {
    let id_of = |entity: &Value| entity.get("id").and_then(Value::as_str).and_then(|id| id.parse::<Identifier>().ok());
    let first = entities.first().and_then(&id_of)?;
    let last = entities.last().and_then(&id_of)?;
    Some((first, last))
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
        .map(|count| usize::try_from(count).map_err(|_negative| TemporalError::from(TemporalInvariant::NegativeMatchCount)))
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
) -> Result<(Vec<NgsiLdWarning>, Vec<Identifier>), ProblemResponse> {
    let Some(join) = RetrievingJoin::from_mode(read.params.join) else {
        return Ok((warnings, Vec::new()));
    };
    let applied = apply_temporal_join(read, join, entities, seed, warnings).await.with_instance(instance)?;
    Ok((applied.warnings, applied.linked_ids))
}
