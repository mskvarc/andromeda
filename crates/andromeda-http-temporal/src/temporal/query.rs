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
        AcceptFormatNoGeo,
        ProblemResponse,
        Response,
        ResultInstanceExt,
        StatusCode,
        complete_history::HistoryCompleteness,
        query::{
            admission::{AdmittedQuery, admit_request},
            forwarding::{QueryScope, TemporalForwarding, forward_temporal_history},
            hub_aggregation::aggregate_at_hub,
            in_effect_gathering::{GatheringRequest, gather_in_effect},
            merged_page::{MergedPageCut, cut_merged_page},
            page::LocalRead,
            request::TemporalQueryRequest,
            request_entity_map::record_query_map,
            response::{TemporalCollectionMetadata, query_response},
            split_contribution::{ContributionMerge, ContributionShape, contributed_entities, merge_with_contributions},
        },
    },
    temporal_answer::matched_histories,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::prefer::PreferHeader;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::{projection::attrs_selection::AttrsAdmission, temporal::contributed_entities::ContributedEntities};
use andromeda_ngsi_ld_render::{response::ResponseRenderer, structural_projection::StructuralProjection};
use andromeda_temporal::{
    error::TemporalError,
    service::{TemporalQueryResult, TemporalQueryWarning, TemporalRange},
    temporal_invariant::TemporalInvariant,
};
use axum::http::Uri;

/// Admitting a decoded query before it runs.
mod admission;
/// A followed pointer of a POST query, resolved to the held query.
pub(crate) mod followed_query;
/// Asking the registered context sources for their part of the history.
pub(crate) mod forwarding;
/// Aggregating here the entities a distributed aggregated query meets in more than one part.
pub(crate) mod hub_aggregation;
/// Gathering what every part holds in effect when a split query's window opens.
mod in_effect_gathering;
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
/// Closing the hub-computed aggregates of one response to the same whole-range period.
mod whole_range_join;

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
        posted,
    } = request;
    let service = &state.temporal;
    let admitted = admit_request(&state, &target, &context, protocol, &mut params, &uri).await?;
    let scope = QueryScope::of(&target, &params);

    let split = state.forwarding.split_entities(params.split_entities);
    let filtering = scope.splitting(split);
    let shape = ContributionShape::of(filtering, &params, state.forwarding.aggregation_gathering());

    // Forwarding runs before the local read, because under `splitEntities`
    // what the sources answered decides which entities the local read exempts
    // from the value, geo and scope filters (clause 5.7.4.4).
    let sources = TemporalForwarding::new(&state.forwarding, &admitted, &headers, split, shape.reach());
    let forwarded = forward_temporal_history(sources, scope, &target, &params, &context, &uri).await?;
    let gathering = GatheringRequest::new(&target, &params, &context, admitted.language.targeting, &uri);
    let in_effect = gather_in_effect(sources, service, gathering, filtering, forwarded.as_ref()).await?;

    let contributed = contributed_entities(filtering, &params, forwarded.as_ref(), in_effect.as_ref(), shape.rendering());
    let local = page.local_read(forwarded.as_ref());
    let read = TemporalHistoryRead {
        service: &state.temporal,
        target: &target,
        params: &params,
        context: &context,
        language_targeting: admitted.language.targeting,
        attrs_admission: AttrsAdmission::ReifiedScopeAdmitted,
    };
    let (local_result, local_total) = query_local_history(&read, &admitted, &local, &contributed, &uri).await?;
    let answers = forwarded.as_ref().map_or(&[][..], |history| history.sources.as_slice());
    let mut completeness = HistoryCompleteness::of(&params, local_result.range, answers).with_instance(&uri)?;
    let contribution = ContributionMerge {
        target: &target,
        params: &params,
        context: &context,
        language_targeting: admitted.language.targeting,
        filtering,
        shape,
        contributed: &contributed,
    };
    let projection = local_result.projection;
    let mut local_entities = local_result.entities;
    let hub_aggregates = aggregate_at_hub(service, sources, gathering, shape, &mut local_entities, forwarded.as_ref()).await?;
    let mut merged = merge_with_contributions(service, contribution, local_entities, forwarded, in_effect, &uri).await?;
    hub_aggregates.replace_in(&mut merged);
    let timeproperty = params.timeproperty_or_default();
    let cut_short = completeness.trim(&mut merged.entities, timeproperty);
    merged.warnings.extend(cut_short);
    // Clause 6.3.17: a query evaluated by tag equality because a reached
    // source lacks extended filtering says so.
    merged.warnings.extend(admitted.language.warning);

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
    // The histories are compacted under the request context once every holder's
    // part has been merged into them (clause 5.7.3); the join then reads the
    // compact roots.
    let bodies = service.compact_histories(entities, &context).await.with_instance(&uri)?;
    let (resolved, warnings) = apply_temporal_join(&read, &bodies, merged.warnings).await.with_instance(&uri)?;
    let linked_ids = resolved.linked_ids();

    let map_created = admitted.map_use.mints();
    let entity_map_uri = record_query_map(&state.entity_maps, &read, admitted.map_use, merged.contributions, linked_ids, &uri).await?;

    let status = response_status(range.as_ref(), map_created);
    // Reach is read from the matching entities alone: a flat clause 4.5.23
    // join appends linked entities that accompany the result set rather than
    // belonging to it.
    let links = page.links(total_count, reach, page_boundary);
    // A posted query is held only when its page points at another page.
    if let Some(posted) = posted.as_deref()
        && !links.is_empty()
    {
        state.posted_queries.hold(posted).await.with_instance(&uri)?;
    }
    // Table 6.18.3.2-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is
    // what renders; clause 6.3.12's representation is what decides how clause
    // 4.3.6.8 reads an Attribute of the answer. Table 6.24.3.1-1 prints no 203
    // row for the POST form, but clause 6.24.3.1 gives it the clause 5.7.4
    // behaviour of the GET and clause 6.3.6 ties the echo to any response
    // carrying a payload body, so both bindings answer the same way.
    let source = ResponseContextSource::new(service.engine(), &context, &state.context_hosting, &api_root);
    let rendering = start_rendering(source, &accept, admitted.prefer, &uri).await?;
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
            links,
        },
        &uri,
    )
}

/// Run the local read of the query under `read` and count its matches, logging
/// the non-fatal per-entity failures the read collected.
async fn query_local_history(
    read: &TemporalHistoryRead<'_>,
    admitted: &AdmittedQuery,
    local: &LocalRead<'_>,
    contributed: &ContributedEntities,
    instance: &Uri,
) -> Result<(TemporalQueryResult, Option<usize>), ProblemResponse> {
    let local_result = read
        .service
        // clone: the query takes ownership, but the settlement still reads the contributed entities
        .query(
            read.target,
            read.params,
            &local.window,
            local.count,
            admitted.reading(read.context),
            contributed.clone(),
        )
        .await
        .with_instance(instance)?;
    log_query_warnings(&local_result.warnings);
    let local_total = local_match_count(local_result.total_count).with_instance(instance)?;
    Ok((local_result, local_total))
}

/// Start the normalized rendering that `accept` and `prefer` negotiate against
/// the context `source` presents.
async fn start_rendering<'a>(
    source: ResponseContextSource<'a>,
    accept: &AcceptFormatNoGeo,
    prefer: PreferHeader,
    instance: &Uri,
) -> Result<ResponseRenderer<'a>, ProblemResponse> {
    source
        .start_rendering_normalized(accept.media_type(), prefer.rendering_preferences())
        .await
        .with_instance(instance)
}

/// The status a temporal query answers with (Table 6.18.3.2-2 and Table
/// 6.24.3.1-1): 206 when the history (the local read's or a context
/// source's) was cut short at `range`, else 201 when an `EntityMap` has been
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
    Some((first.clone(), last.clone()))
}

/// Record the non-fatal per-entity failures a temporal query collected.
fn log_query_warnings(warnings: &[TemporalQueryWarning]) {
    for warning in warnings {
        tracing::debug!(entity_id = %warning.entity_id, reason = %warning.reason, "Temporal query warning");
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
