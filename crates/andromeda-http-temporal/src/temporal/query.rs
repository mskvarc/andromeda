//! HTTP handling for temporal query (clause 5.7.4).

use crate::{
    history_read::TemporalHistoryRead,
    join::apply_temporal_join,
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
            forwarding::{TemporalForwarding, forward_temporal_history},
            merged_history::merge_forwarded_history,
            merged_page::{MergedPageCut, cut_merged_page, defer_split_filters, hold_page_until_merge},
            request_entity_map::{record_query_map, resolve_query_map},
            response::{TemporalCollectionMetadata, query_response, simplified_collection},
        },
    },
};
use andromeda_federation::{config::SplitEntities, request::TriggeringRequest};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::{error::TemporalError, linked::candidates::TemporalLinkedCandidate, service::TemporalQueryWarning};
use axum::http::Uri;
use serde_json::Value;

/// Asking the registered context sources for their part of the history.
mod forwarding;
/// Folding what those sources returned into the locally read history.
mod merged_history;
/// The page the query answers with, cut from the merged history.
mod merged_page;
/// The `EntityMap` the query runs under.
mod request_entity_map;
/// Rendering the collection the query answers with.
mod response;

pub async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    protocol.extensions().apply_to_temporal_query(&mut params).with_instance(&uri)?;
    let entity_map_header = protocol.entity_map;
    let key_values = params.is_simplified();

    let map_use = resolve_query_map(&state.entity_maps, &target, &mut params, entity_map_header.as_ref(), &uri).await?;

    if !target.permits_federation() {
        params.local = Some(true);
    }
    params.validate(true).with_instance(&uri)?;

    let is_local = !target.permits_federation() || params.is_local();
    let split = state.forwarding.split_entities(params.split_entities);
    // Clause 5.7.4.4: only a distributed read can meet an entity whose
    // attributes are spread over several sources, so a local one always sees
    // complete entities and defers nothing.
    let filtering = if is_local { SplitEntities::Complete } else { split };

    // Distributed forwarding: query matching context sources unless
    // local=true. Forwarding reads the query as the client wrote it, so it
    // runs before the split-entities filters are held back from the local
    // query below.
    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(&headers),
    };
    let forwarded = if is_local {
        None
    } else {
        Some(
            forward_temporal_history(
                TemporalForwarding {
                    forwarding: &state.forwarding,
                    triggering,
                    split,
                    map_use: &map_use,
                },
                service.engine(),
                &target,
                &params,
                &context,
                &uri,
            )
            .await?,
        )
    };

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

    // Table 6.18.3.2-2: 201 when an EntityMap has been (re)created, 206 when
    // the local history was cut short by the broker's own lastN cap.
    let status = if local_result.any_truncated && local_result.range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else if map_created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    query_response(
        accept,
        &context,
        values,
        TemporalCollectionMetadata {
            status,
            warnings,
            total_count,
            range: local_result.range,
            entity_map_uri,
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
