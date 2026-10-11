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
    temporal::{
        ProblemResponse,
        Response,
        ResultInstanceExt,
        StatusCode,
        query::{
            admission::{AdmittedQuery, admit_request},
            complete_history::HistoryCompleteness,
            forwarding::{QueryScope, TemporalForwarding, forward_temporal_history},
            merged_page::{MergedPageCut, cut_merged_page},
            request::TemporalQueryRequest,
            request_entity_map::record_query_map,
            response::{TemporalCollectionMetadata, query_response},
            split_contribution::{ContributionMerge, ContributionShape, contributed_entities, merge_with_contributions},
        },
    },
    temporal_answer::matched_histories,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_render::structural_projection::StructuralProjection;
use andromeda_temporal::{
    error::TemporalError,
    service::{TemporalQueryWarning, TemporalRange},
    temporal_invariant::TemporalInvariant,
};

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
pub(crate) mod page;
/// The query-string binding, and the request both bindings hand the operation.
pub(crate) mod request;
/// The `EntityMap` the query runs under.
mod request_entity_map;
/// Rendering the collection the query answers with.
mod response;
/// The entities a source contributed to a split query, and the filters decided over them.
mod split_contribution;

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
        page,
    } = request;
    let service = &state.temporal;
    let AdmittedQuery { prefer, incoming_via, map_use } = admit_request(&state, &target, &context, protocol, &mut params, &uri).await?;
    let scope = QueryScope::of(&target, &params);

    let split = state.forwarding.split_entities(params.split_entities);
    let filtering = scope.splitting(split);
    let shape = ContributionShape::of(filtering, &params);

    // Forwarding runs before the local read, because under `splitEntities`
    // what the sources answered decides which entities the local read exempts
    // from the value, geo and scope filters (clause 5.7.4.4).
    let sources = TemporalForwarding::new(&state.forwarding, incoming_via.as_ref(), &headers, split, &map_use, shape.reach());
    let forwarded = forward_temporal_history(sources, scope, service.engine(), &target, &params, &context, &uri).await?;

    let contributed = contributed_entities(filtering, &params, forwarded.as_ref(), shape.rendering());
    let local = page.local_read(forwarded.as_ref());

    let local_result = service
        // clone: the settlement below reads the same contributed entities the local read is exempting
        .query(&target, &params, &local.window, local.count, &context, contributed.clone())
        .await
        .with_instance(&uri)?;
    log_query_warnings(&local_result.warnings);
    let local_total = local_match_count(local_result.total_count).with_instance(&uri)?;
    let completeness = HistoryCompleteness::of(&params, local_result.range, forwarded.as_ref()).with_instance(&uri)?;
    let contribution = ContributionMerge {
        target: &target,
        params: &params,
        context: &context,
        filtering,
        shape,
        contributed: &contributed,
    };
    let projection = local_result.projection;
    let mut merged = merge_with_contributions(service, contribution, local_result.entities, forwarded, &uri).await?;
    let timeproperty = params.timeproperty_or_default();
    completeness.trim(&mut merged.entities, timeproperty);

    let (entities, total_count, reach) = cut_merged_page(
        merged.entities,
        MergedPageCut {
            window: local.merged,
            count: page.count(),
            local_total,
            local_reach: local_result.reach,
        },
        &params,
    );
    // Clause 6.3.10: the `Content-Range` describes the history the page's own
    // entities carry, read before a flat join appends the linked ones.
    let range = completeness.range(&entities, timeproperty);
    // Clause 5.5.9.1: the boundary a cursor pointer walks from is the first and
    // last identifiers of the matching page, captured here before the flat join
    // appends the linked entities the page must not point past.
    let page_boundary = element_boundary(&entities);

    // Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and an
    // absent `join` retrieve no linked entity at all, so neither reaches the
    // traversal. The join runs after forwarding and after the `splitEntities`
    // filters were decided over the merge, so the roots it walks are the ones
    // the client will actually be shown.
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
    // The histories are compacted under the request context once every holder's
    // part has been merged into them (clause 5.7.3); the join then reads the
    // compact roots.
    let bodies = service.compact_histories(entities, &context).await.with_instance(&uri)?;
    let (resolved, warnings) = apply_temporal_join(&read, &bodies, merged.warnings).await.with_instance(&uri)?;
    let linked_ids = resolved.linked_ids();

    let map_created = map_use.mints();
    let entity_map_uri = record_query_map(&state.entity_maps, &read, map_use, merged.contributions, linked_ids, &uri).await?;

    let status = response_status(range.as_ref(), map_created);
    // Table 6.18.3.2-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is
    // what renders; clause 6.3.12's representation is what decides how clause
    // 4.3.6.8 reads an Attribute of the answer. Table 6.24.3.1-1 prints no 203
    // row for the POST form, but clause 6.24.3.1 gives it the clause 5.7.4
    // behaviour of the GET and clause 6.3.6 ties the echo to any response
    // carrying a payload body, so both bindings answer the same way.
    let rendering = ResponseContextSource::new(service.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), prefer.rendering_preferences())
        .await
        .with_instance(&uri)?;
    let plan = resolved.plan();
    let answer = matched_histories(&bodies, plan.as_ref(), Some(StructuralProjection::from(&projection)));
    query_response(
        accept,
        rendering,
        answer,
        &params,
        TemporalCollectionMetadata {
            status,
            warnings,
            total_count,
            range,
            entity_map_uri,
            page,
            reach,
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
fn element_boundary(entities: &[TemporalHistory]) -> Option<(Identifier, Identifier)> {
    let first = entities.first().and_then(TemporalHistory::id)?;
    let last = entities.last().and_then(TemporalHistory::id)?;
    Some((first.clone(), last.clone())) // clone: the cursor pointers own the identifiers they are computed from
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
