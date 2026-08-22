//! HTTP handling for temporal query.

use crate::{
    error::single_source_declined,
    join::apply_temporal_join,
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        ContextUrl,
        ContextualExpander,
        CsrModeFilter,
        DeferredSplitFilters,
        ForwardTemporalQueryRequest,
        ForwardingMode,
        ForwardingOperation,
        IntoResponse,
        NgsiLdCollectionResponse,
        NgsiLdProtocolHeaders,
        OriginalUri,
        PreferHeader,
        ProblemResponse,
        Response,
        ResponsePayload,
        ResultInstanceExt,
        StatusCode,
        TemporalQueryParameters,
        TemporalQueryResult,
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
use andromeda_entity_map::{
    fingerprint::{FingerprintedRequest, fingerprint_of},
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_federation::{
    config::SplitEntities,
    entity_result::temporal_contributions_of,
    outcome::{ForwardReadOutcome, SourceContribution},
    request::{EntityMapInUse, TriggeringRequest},
    temporal_last_n::truncate_to_last_n,
    temporal_result::merge_temporal_query_results,
};
use andromeda_http_extraction::tenant_state;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_http_response::content_range::build_content_range;
use andromeda_ngsi_ld_request::{join_traversal::RetrievingJoin, query::SharedQueryParams};
use andromeda_temporal::error::TemporalError;
use axum::http::HeaderMap;
use tracing::warn;

pub async fn query(
    tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    headers: HeaderMap,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    protocol.extensions().apply_to_temporal_query(&mut params).with_instance(&uri)?;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;
    let key_values = params.is_simplified();

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, query temporal entities from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        state.snapshots.validate(&snapshot_id).await.with_instance(&uri)?;

        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let limit = params.limit.unwrap_or(100) as i64;
        let offset = params.offset.unwrap_or(0) as i64;

        let (entities, total) = state
            .snapshots
            .entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_instance(&uri)?;

        let values = if key_values {
            TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(entities))
                .render()
                .with_instance(&uri)?
                .into_collection()
                .with_instance(&uri)?
        } else {
            entities
        };
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Collection(values))
            .render()
            .with_instance(&uri)?;
        let response = NgsiLdCollectionResponse::ok(rendered)
            .with_snapshot_uri(snapshot_id)
            .with_total_count(total as usize);
        return Ok(response.into_response());
    }

    // Clause 5.5.14: a referenced live EntityMap fixes the entities the query
    // considers and the sources it contacts, and holds the query to the
    // creating request's filters (the temporal window excepted); an expired
    // or unknown one yields no inference, so a fresh map is created instead.
    // The fingerprint is taken before the query is narrowed below.
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryTemporal(&params)).with_instance(&uri)?;
    let map_use = state
        .entity_maps
        .resolve_use(
            entity_map_header.as_ref().map(NgsiLdEntityMap::as_inner),
            EntityMapFlag::from_parameter(params.entity_map),
            &fingerprint,
            params.entity_map_lifetime.as_ref(),
        )
        .await
        .with_instance(&uri)?;
    if let Some(map) = map_use.reused() {
        params.id = Some(EntityMapService::scope_query_ids(map, params.id.as_deref()));

        // EntityMap-scoped temporal queries may omit temporal bounds; default to
        // an "all history up to far future" window so scoped follow-up queries succeed.
        if params.timerel.is_none() && params.time_at.is_none() {
            params.timerel = Some(TimeRel::Before);
            params.time_at = Some(
                chrono::DateTime::parse_from_rfc3339("9999-12-31T23:59:59Z")
                    .map(|instant| instant.with_timezone(&chrono::Utc))
                    .map_err(|source| TemporalError::validation(format!("Invalid entity-map temporal horizon: {source}")))
                    .with_instance(&uri)?,
            );
        }
    }

    params.validate(true).with_instance(&uri)?;

    let is_local = params.is_local();
    let split = state.forwarding.split_entities(params.split_entities);
    let split_entities = !is_local && split == SplitEntities::Split;

    // Distributed forwarding: query matching context sources unless
    // local=true. Forwarding reads the query as the client wrote it, so it
    // runs before the split-entities filters are held back from the local
    // query below.
    let forwarded = if is_local {
        None
    } else {
        // The context source filter selects registrations in its expanded
        // form; the raw expression is forwarded as the client wrote it
        // (clause 5.7.4.4 propagates it into discovery).
        let csf_expr = match &params.csf {
            Some(expr) => {
                let prepared_context = service.engine().prepare_term_context(&context);
                let expander = ContextualExpander::new(service.engine(), &prepared_context);
                Some(expand_q_expr(expr.clone(), &expander).await.with_instance(&uri)?) // clone: the raw filter stays in the query for forwarding
            }
            None => None,
        };
        let outcome = state
            .forwarding
            .forward_temporal_query(ForwardTemporalQueryRequest {
                client_query: &params,
                split,
                context: &context,
                operation: ForwardingOperation::QueryTemporalEntity,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
                csf_expr: csf_expr.as_ref(),
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::from(&map_use),
            })
            .await
            .unwrap_or_else(|error| {
                warn!(%error, "context source forwarding failed during the temporal query");
                ForwardReadOutcome::empty()
            });
        if let Some(status) = outcome.sources.single_source_status {
            return Err(single_source_declined(status, &uri));
        }
        Some(outcome.results)
    };

    // splitEntities (spec 5.7.4.4): defer q/geoQ/scopeQ filtering to post-merge
    let deferred = if split_entities {
        DeferredSplitFilters {
            q: params.q.take(),
            geo: params.geo.0.take(),
            scope_q: params.scope_q.take(),
        }
    } else {
        DeferredSplitFilters::default()
    };

    // Clause 5.5.9: once sources contribute entities, the page is cut from
    // the merged collection rather than from the local rows, so the local
    // query runs unpaged and the requested page is applied after the merge.
    let page_limit = params.effective_limit();
    let page_offset = params.effective_offset();
    let paginate_after_merge = forwarded
        .as_ref()
        .is_some_and(|results| results.iter().any(|result| !result.entities.is_empty()));
    if paginate_after_merge {
        params.limit = Some(u32::MAX);
        params.offset = Some(0);
    }

    let result = service.query(&params, &context).await.with_instance(&uri)?;
    for warning in &result.warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal query warning");
    }
    let TemporalQueryResult {
        entities: local_entities,
        total_count: local_total,
        any_truncated,
        range,
        warnings: _,
        linked_candidates,
    } = result;

    let mut contributions: Vec<SourceContribution> = Vec::new();
    let (mut entities, forwarding_warnings) = match forwarded {
        None => (local_entities, Vec::new()),
        Some(forwarded) => {
            let warnings = collect_temporal_warnings(&forwarded);
            // Capture which registration returned which entity, and with
            // which map, before the merge (clauses 5.2.39 and 5.5.14).
            contributions = temporal_contributions_of(&forwarded).collect();
            let forwarded = if split_entities {
                forwarded
                    .into_iter()
                    .map(|mut result| {
                        if result.mode == ForwardingMode::Auxiliary {
                            result.mode = ForwardingMode::Inclusive;
                        }
                        result
                    })
                    .collect()
            } else {
                forwarded
            };
            let merged = merge_temporal_query_results(local_entities, forwarded);
            (merged, warnings)
        }
    };

    // splitEntities post-merge filtering: apply deferred q/geoQ/scopeQ (spec 5.7.4.4)
    if split_entities {
        let q_expr = if let Some(q_expr) = deferred.q {
            let prepared_context = service.engine().prepare_term_context(&context);
            let expander = ContextualExpander::new(service.engine(), &prepared_context);
            Some(expand_q_expr(q_expr, &expander).await.with_instance(&uri)?)
        } else {
            None
        };
        let prepared_geo_context = service.engine().prepare_term_context(&context);
        let geo_expander = ContextualExpander::new(service.engine(), &prepared_geo_context);
        let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &geo_expander).await.with_instance(&uri)?;
        let compiled_scope = deferred.scope_q;

        let mut filtered = Vec::with_capacity(entities.len());
        for entity in entities {
            let expanded = match service.engine().expand(entity.clone(), &context).await {
                // clone: `expand` consumes `Value`, and we must keep original compact value for response
                Ok(value) => value,
                Err(_) => continue,
            };

            let q_ok = q_expr.as_ref().is_none_or(|expr| evaluate(expr, &expanded));
            let geo_ok = compiled_geo.as_ref().is_none_or(|geo| matches_geo_query(geo, &expanded));
            let scope_ok = compiled_scope.as_ref().is_none_or(|scope| matches_scope_query(scope, &expanded));

            if q_ok && geo_ok && scope_ok {
                filtered.push(entity);
            }
        }
        entities = filtered;
    }

    // Clause 5.7.4.4 (S8): `lastN` bounds the instances of the aggregated
    // history, so it is applied once more over the merged entities; each
    // source only bounded its own part. Then clause 5.5.9: the page of the
    // merged, filtered collection.
    let total_count = if paginate_after_merge {
        if let Some(last_n) = params.last_n {
            let timeproperty = params.timeproperty_or_default();
            for entity in &mut entities {
                truncate_to_last_n(entity, last_n, timeproperty);
            }
        }
        let total = params.count.unwrap_or(false).then_some(entities.len() as i64);
        entities = entities
            .into_iter()
            .skip(usize::try_from(page_offset).unwrap_or(usize::MAX))
            .take(usize::try_from(page_limit).unwrap_or(usize::MAX))
            .collect();
        total
    } else {
        local_total
    };

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
    let (forwarding_warnings, linked_ids) = match RetrievingJoin::from_mode(params.join) {
        Some(join) => {
            let applied = apply_temporal_join(service, join, &mut entities, &linked_candidates, &params, &context, forwarding_warnings)
                .await
                .with_instance(&uri)?;
            (applied.warnings, applied.linked_ids)
        }
        None => (forwarding_warnings, Vec::new()),
    };

    // Clause 5.5.14: a reused map records the maps its sources answered
    // with; a minted one (clause 5.7.4.4 `entityMap=true`) covers every
    // matching entity, not only the page.
    let map_created = map_use.mints();
    let entity_map_uri = match map_use {
        EntityMapUse::Reuse(map) => {
            let links = contributions
                .iter()
                .filter_map(|contribution| contribution.linked_map.as_ref().map(|linked| (&contribution.registration, linked)));
            state.entity_maps.record_linked_maps(&map.id, links).await.with_instance(&uri)?;
            Some(map.id)
        }
        EntityMapUse::Mint(mut map) => {
            let mut local_ids = service.query_ids(&params, &context).await.with_instance(&uri)?;
            // Linked entities the join resolved belong to the unitary request too (clause 5.5.14).
            local_ids.extend(linked_ids);
            local_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
            local_ids.dedup();
            map.add_local_entities(local_ids);
            for contribution in contributions {
                map.add_contribution(&contribution.registration, contribution.entity_ids, contribution.linked_map);
            }
            state.entity_maps.store(&map).await.with_instance(&uri)?;
            Some(map.id)
        }
        EntityMapUse::None => None,
    };

    // Table 6.18.3.2-2: 201 when an EntityMap has been (re)created.
    let status = if map_created { StatusCode::CREATED } else { StatusCode::OK };

    let values = if key_values {
        TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(entities))
            .render()
            .with_instance(&uri)?
            .into_collection()
            .with_instance(&uri)?
    } else {
        entities
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(values))
        .render()
        .with_instance(&uri)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(forwarding_warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    if any_truncated && range.is_some() {
        response = response.with_status(StatusCode::PARTIAL_CONTENT);
    }
    if let Some(range) = range
        && let Some(content_range) = build_content_range(&range.interval, range.observed, range.partition_total, range.last_n_origin)
    {
        response = response.with_content_range(content_range);
    }
    if let Some(ref map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri.clone()); // clone: owned String needed for response header; map_uri borrowed by ref pattern
    }
    Ok(response.into_response())
}
