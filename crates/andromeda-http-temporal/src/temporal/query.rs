//! HTTP handling for temporal query.

use crate::{
    error::single_source_declined,
    history_read::TemporalHistoryRead,
    join::apply_temporal_join,
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        ContextualExpander,
        CsrModeFilter,
        DeferredSplitFilters,
        ForwardTemporalQueryRequest,
        ForwardingMode,
        ForwardingOperation,
        IntoResponse,
        NgsiLdCollectionResponse,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        PreferHeader,
        ProblemResponse,
        Response,
        ResponsePayload,
        ResultInstanceExt,
        StatusCode,
        TemporalQueryParameters,
        TemporalSimplifyPayload,
        TemporalSimplifyRenderer,
        TenantState,
        TimeRel,
        ValidatedQuery,
        collect_temporal_warnings,
        compile_geo_query,
        evaluate,
        expand_q_expr,
        matches_geo_query,
        matches_scope_query,
        start_rendering,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    fingerprint::{FingerprintedRequest, fingerprint_of},
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_federation::{
    FederationService,
    config::SplitEntities,
    entity_result::temporal_contributions_of,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities, SourceContribution},
    request::{EntityMapInUse, TriggeringRequest},
    temporal_last_n::truncate_to_last_n,
    temporal_result::merge_temporal_query_results,
};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::{ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use andromeda_http_response::content_range::build_content_range;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
use andromeda_ngsi_ld_request::{join_traversal::RetrievingJoin, query::SharedQueryParams};
use andromeda_temporal::{
    error::TemporalError,
    linked::candidates::TemporalLinkedCandidate,
    service::{TemporalQueryWarning, TemporalRange},
};
use axum::http::Uri;
use serde_json::Value;
use tracing::warn;

pub async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    accept: AcceptFormatNoGeo,
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

/// Resolve the Entity Map the query runs under and hold the query to it.
///
/// Clause 5.5.14: a referenced live map fixes the entities the query considers
/// and the sources it contacts, and holds the query to the creating request's
/// filters (the temporal window excepted); an expired or unknown one yields no
/// inference, so a fresh map is created instead. The fingerprint is taken
/// before the query is narrowed. A map-scoped follow-up may omit the temporal
/// window, which then stands for all history up to a far-future horizon.
///
/// # Errors
///
/// The classified failure of fingerprinting the request or of reading the map
/// store, and [`TemporalError::Validation`] when the far-future horizon cannot
/// be parsed.
async fn resolve_query_map(
    entity_maps: &EntityMapService,
    target: &ContextTarget,
    params: &mut TemporalQueryParameters,
    referenced: Option<&NgsiLdEntityMap>,
    instance: &Uri,
) -> Result<EntityMapUse, ProblemResponse> {
    if !target.permits_federation() {
        return Ok(EntityMapUse::None);
    }
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryTemporal(params)).with_instance(instance)?;
    let map_use = entity_maps
        .resolve_use(
            referenced.map(NgsiLdEntityMap::as_inner),
            EntityMapFlag::from_parameter(params.entity_map),
            &fingerprint,
            params.entity_map_lifetime.as_ref(),
        )
        .await
        .with_instance(instance)?;
    if let Some(map) = map_use.reused() {
        params.id = Some(EntityMapService::scope_query_ids(map, params.id.as_deref()));
        if params.timerel.is_none() && params.time_at.is_none() {
            params.timerel = Some(TimeRel::Before);
            params.time_at = Some(
                chrono::DateTime::parse_from_rfc3339("9999-12-31T23:59:59Z")
                    .map(|instant| instant.with_timezone(&chrono::Utc))
                    .map_err(|source| TemporalError::validation(format!("Invalid entity-map temporal horizon: {source}")))
                    .with_instance(instance)?,
            );
        }
    }
    Ok(map_use)
}

/// How a temporal query reaches the context sources that hold the rest of the
/// history: the federation service that contacts them, the triggering request
/// every forwarded copy replays (clause 6.3.18), the `splitEntities` mode they
/// are asked under (clause 5.7.4.4), and the Entity Map that fixes which of
/// them are contacted (clause 5.5.14).
struct TemporalForwarding<'a> {
    /// The service that contacts every matching registration.
    forwarding: &'a FederationService,
    /// The triggering request every forwarded copy replays, so a forwarding
    /// loop is detected rather than followed.
    triggering: TriggeringRequest<'a>,
    /// Whether an entity's attributes may be spread over several sources.
    split: SplitEntities,
    /// The Entity Map the query runs under.
    map_use: &'a EntityMapUse,
}

/// Ask every matching context source for its part of the history.
///
/// The context source filter selects registrations in its expanded form; the
/// raw expression travels on as the client wrote it, since clause 5.7.4.4
/// propagates it into discovery. A source that cannot be reached contributes a
/// clause 6.3.17 warning rather than failing the operation.
///
/// # Errors
///
/// The classified failure of expanding the context source filter, and the
/// Table 6.4.3.2-3 `NotImplemented` answer when the one source holding all the
/// data declined the requested representation.
async fn forward_temporal_history(
    sources: TemporalForwarding<'_>,
    engine: &JsonLdEngine,
    target: &ContextTarget,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    instance: &Uri,
) -> Result<Vec<ForwardedTemporalEntities>, ProblemResponse> {
    let TemporalForwarding {
        forwarding,
        triggering,
        split,
        map_use,
    } = sources;
    let csf_expr = match &params.csf {
        Some(expr) => {
            let prepared_context = engine.prepare_term_context(context);
            let expander = ContextualExpander::new(engine, &prepared_context);
            Some(expand_q_expr(expr.clone(), &expander).await.with_instance(instance)?) // clone: the raw filter stays in the query for forwarding
        }
        None => None,
    };
    let outcome = forwarding
        .forward_temporal_query(
            target,
            ForwardTemporalQueryRequest {
                client_query: params,
                split,
                context,
                operation: ForwardingOperation::QueryTemporalEntity,
                triggering,
                csf_expr: csf_expr.as_ref(),
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::from(map_use),
                source_page: None,
            },
        )
        .await
        .unwrap_or_else(|error| {
            warn!(%error, "context source forwarding failed during the temporal query");
            ForwardReadOutcome::empty()
        });
    match outcome.sources.single_source_status {
        Some(status) => Err(single_source_declined(status, instance)),
        None => Ok(outcome.results),
    }
}

/// Hold back the filters no single context source can decide on its own.
///
/// Clause 5.7.4.4: under `splitEntities` an entity's attributes may be spread
/// over several sources, so `q`, the geoquery and the scope query leave the
/// local query and are evaluated once over the merged result instead.
fn defer_split_filters(params: &mut TemporalQueryParameters, filtering: SplitEntities) -> DeferredSplitFilters {
    match filtering {
        SplitEntities::Split => DeferredSplitFilters {
            q: params.q.take(),
            geo: params.geo.0.take(),
            scope_q: params.scope_q.take(),
        },
        SplitEntities::Complete => DeferredSplitFilters::default(),
    }
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

/// The merged history of a temporal query, and what the sources contributed to it.
struct MergedTemporalHistory {
    /// The local rows folded together with what every contacted source returned.
    entities: Vec<Value>,
    /// The clause 6.3.17 warnings the forwarding produced.
    warnings: Vec<NgsiLdWarning>,
    /// Which registration returned which entity, and under which map, captured
    /// before the merge erased the boundary (clauses 5.2.39 and 5.5.14).
    contributions: Vec<SourceContribution>,
}

/// Fold what the context sources returned into the local rows.
///
/// Clause 5.7.4.4: under `splitEntities` an auxiliary source holds part of an
/// entity the broker also holds, so its contribution joins the aggregation
/// instead of only filling gaps.
fn merge_forwarded_history(local_entities: Vec<Value>, forwarded: Option<Vec<ForwardedTemporalEntities>>, filtering: SplitEntities) -> MergedTemporalHistory {
    let Some(forwarded) = forwarded else {
        return MergedTemporalHistory {
            entities: local_entities,
            warnings: Vec::new(),
            contributions: Vec::new(),
        };
    };
    let warnings = collect_temporal_warnings(&forwarded);
    let contributions: Vec<SourceContribution> = temporal_contributions_of(&forwarded).collect();
    let forwarded: Vec<ForwardedTemporalEntities> = match filtering {
        SplitEntities::Split => forwarded
            .into_iter()
            .map(|mut result| {
                if result.mode == ForwardingMode::Auxiliary {
                    result.mode = ForwardingMode::Inclusive;
                }
                result
            })
            .collect(),
        SplitEntities::Complete => forwarded,
    };
    MergedTemporalHistory {
        entities: merge_temporal_query_results(local_entities, forwarded),
        warnings,
        contributions,
    }
}

/// Keep only the merged entities the deferred `splitEntities` filters accept.
///
/// Clause 5.7.4.4: `q`, the geoquery and the scope query are evaluated once
/// over the re-assembled entities, because no single source held enough of an
/// entity to decide them. An entity that cannot be expanded cannot be matched
/// against an expanded filter, so it does not reach the response.
///
/// # Errors
///
/// The classified failure of expanding the deferred `q` or geoquery.
async fn retain_split_matches(
    engine: &JsonLdEngine,
    context: &ContextSpec,
    deferred: DeferredSplitFilters,
    entities: Vec<Value>,
    instance: &Uri,
) -> Result<Vec<Value>, ProblemResponse> {
    let q_expr = match deferred.q {
        Some(q_expr) => {
            let prepared_context = engine.prepare_term_context(context);
            let expander = ContextualExpander::new(engine, &prepared_context);
            Some(expand_q_expr(q_expr, &expander).await.with_instance(instance)?)
        }
        None => None,
    };
    let prepared_geo_context = engine.prepare_term_context(context);
    let geo_expander = ContextualExpander::new(engine, &prepared_geo_context);
    let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &geo_expander).await.with_instance(instance)?;
    let compiled_scope = deferred.scope_q;

    let mut retained = Vec::with_capacity(entities.len());
    for entity in entities {
        // clone: `expand` consumes the value, and the compact original is what the response carries
        let Ok(expanded) = engine.expand(entity.clone(), context).await else {
            continue;
        };

        let q_ok = q_expr.as_ref().is_none_or(|expr| evaluate(expr, &expanded));
        let geo_ok = compiled_geo.as_ref().is_none_or(|geo| matches_geo_query(geo, &expanded));
        let scope_ok = compiled_scope.as_ref().is_none_or(|scope| matches_scope_query(scope, &expanded));

        if q_ok && geo_ok && scope_ok {
            retained.push(entity);
        }
    }
    Ok(retained)
}

/// Bound the merged history and cut the requested page from it.
///
/// Clause 5.7.4.4: `lastN` bounds the instances of the aggregated history, so
/// it is applied once more over the merged entities; each source only bounded
/// its own part. Clause 5.5.9 then takes the page from the merged collection
/// rather than from the local rows, and clause 6.3.13 counts the merged
/// matches rather than the page.
fn merged_page(entities: Vec<Value>, params: &TemporalQueryParameters, limit: u32, offset: u32) -> (Vec<Value>, Option<usize>) {
    let mut bounded = entities;
    if let Some(last_n) = params.last_n {
        let timeproperty = params.timeproperty_or_default();
        for entity in &mut bounded {
            truncate_to_last_n(entity, last_n, timeproperty);
        }
    }
    let bounded = bounded;
    let total = params.count.unwrap_or(false).then_some(bounded.len());
    let page = bounded
        .into_iter()
        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .collect();
    (page, total)
}

/// Hold the requested page back until after the merge when context sources
/// contributed entities, returning the bounds the merged collection is cut to.
///
/// Clause 5.5.9 cuts the page from the merged collection rather than from the
/// local rows, so the local query has to run unpaged.
fn hold_page_until_merge(params: &mut TemporalQueryParameters, forwarded: Option<&[ForwardedTemporalEntities]>) -> Option<(u32, u32)> {
    let contributed = forwarded.is_some_and(|results| results.iter().any(|result| !result.entities.is_empty()));
    if !contributed {
        return None;
    }
    let bounds = (params.effective_limit(), params.effective_offset());
    params.limit = Some(u32::MAX);
    params.offset = Some(0);
    Some(bounds)
}

/// What the local query held back until the context sources had answered.
///
/// Under `splitEntities` no single source held enough of an entity to decide
/// `q`, the geoquery or the scope query (clause 5.7.4.4), and clause 5.5.9
/// cuts the page from the merged collection rather than from the local rows.
/// Both decisions are therefore taken here, over the aggregation.
struct MergedPageCut {
    /// The filters that left the local query and are evaluated over the merge.
    deferred: DeferredSplitFilters,
    /// Whether an entity's attributes may be spread over several sources,
    /// which decides whether the deferred filters apply at all.
    filtering: SplitEntities,
    /// The page bounds withheld while the merge was pending, absent when no
    /// source contributed and the local query already cut the page.
    bounds: Option<(u32, u32)>,
    /// The match count the local read reported, which stands when no page was
    /// withheld.
    local_total: Option<usize>,
}

/// Apply the deferred `splitEntities` filters (clause 5.7.4.4) to the merged
/// collection and cut the page the client asked for.
///
/// # Errors
///
/// The classified failure when a deferred filter cannot be compiled or
/// evaluated against a merged entity.
async fn cut_merged_page(
    engine: &JsonLdEngine,
    context: &ContextSpec,
    merged_entities: Vec<Value>,
    cut: MergedPageCut,
    params: &TemporalQueryParameters,
    instance: &Uri,
) -> Result<(Vec<Value>, Option<usize>), ProblemResponse> {
    let MergedPageCut {
        deferred,
        filtering,
        bounds,
        local_total,
    } = cut;
    let entities = match filtering {
        SplitEntities::Split => retain_split_matches(engine, context, deferred, merged_entities, instance).await?,
        SplitEntities::Complete => merged_entities,
    };
    match bounds {
        Some((limit, offset)) => Ok(merged_page(entities, params, limit, offset)),
        None => Ok((entities, local_total)),
    }
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

/// Record the Entity Map the query ran under and return its location.
///
/// Clause 5.5.14: a reused map records the maps its sources answered with; a
/// minted one (clause 5.7.4.4 `entityMap=true`) covers every matching entity
/// rather than only the page, and the Linked Entities a join resolved belong
/// to the same unitary request.
///
/// # Errors
///
/// The classified failure of listing the matching identifiers or of writing to
/// the map store.
async fn record_query_map(
    entity_maps: &EntityMapService,
    read: &TemporalHistoryRead<'_>,
    map_use: EntityMapUse,
    contributions: Vec<SourceContribution>,
    linked_ids: Vec<NgsiLdUri>,
    instance: &Uri,
) -> Result<Option<NgsiLdUri>, ProblemResponse> {
    match map_use {
        EntityMapUse::Reuse(map) => {
            let links = contributions
                .iter()
                .filter_map(|contribution| contribution.linked_map.as_ref().map(|linked| (&contribution.registration, linked)));
            entity_maps.record_linked_maps(&map.id, links).await.with_instance(instance)?;
            Ok(Some(map.id))
        }
        EntityMapUse::Mint(mut map) => {
            let mut local_ids = read.service.query_ids(read.target, read.params, read.context).await.with_instance(instance)?;
            local_ids.extend(linked_ids);
            local_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
            local_ids.dedup();
            map.add_local_entities(local_ids);
            for contribution in contributions {
                map.add_contribution(&contribution.registration, contribution.entity_ids, contribution.linked_map);
            }
            entity_maps.store(&map).await.with_instance(instance)?;
            Ok(Some(map.id))
        }
        EntityMapUse::None => Ok(None),
    }
}

/// Project a temporal collection to its `keyValues` representation.
///
/// # Errors
///
/// The classified failure of the simplifying renderer.
fn simplified_collection(entities: Vec<Value>, instance: &Uri) -> Result<Vec<Value>, ProblemResponse> {
    TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(entities))
        .render()
        .with_instance(instance)?
        .into_collection()
        .with_instance(instance)
}

/// The metadata a temporal query's response carries beside its rendered body.
struct TemporalCollectionMetadata {
    /// Table 6.18.3.2-2: 201 when an Entity Map has been (re)created, 206 when
    /// the local history was cut short, 200 otherwise.
    status: StatusCode,
    /// The clause 6.3.17 `NGSILD-Warning` entries the read collected.
    warnings: Vec<NgsiLdWarning>,
    /// The clause 6.3.13 `NGSILD-Results-Count` of the matches, when asked for.
    total_count: Option<usize>,
    /// The `Content-Range` of a history the local read cut short.
    range: Option<TemporalRange>,
    /// The Entity Map the query ran under (clause 5.5.14).
    entity_map_uri: Option<NgsiLdUri>,
}

/// Render the queried collection and assemble the response around it.
///
/// # Errors
///
/// The classified rendering failure when the negotiated representation cannot
/// be produced.
fn query_response(
    accept: AcceptFormatNoGeo,
    context: &ContextSpec,
    entities: Vec<Value>,
    metadata: TemporalCollectionMetadata,
    instance: &Uri,
) -> Result<Response, ProblemResponse> {
    let TemporalCollectionMetadata {
        status,
        warnings,
        total_count,
        range,
        entity_map_uri,
    } = metadata;
    let rendered = start_rendering(accept.media_type(), context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(entities))
        .render()
        .with_instance(instance)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count);
    }
    if let Some(range) = range
        && let Some(content_range) = build_content_range(&range.interval, range.observed, range.partition_total, range.last_n_origin)
    {
        response = response.with_content_range(content_range);
    }
    if let Some(map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri);
    }
    Ok(response.into_response())
}
