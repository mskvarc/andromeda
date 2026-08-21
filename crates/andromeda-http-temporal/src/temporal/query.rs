//! HTTP handling for temporal query.

use crate::{
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
        Uri,
        ValidatedQuery,
        Value,
        collect_temporal_warnings,
        compile_geo_query,
        evaluate,
        expand_q_expr,
        matches_geo_query,
        matches_scope_query,
        start_rendering,
    },
};
use andromeda_entity_map::service::{EntityMapService, ReferencedEntityMap};
use andromeda_federation::{request::TriggeringRequest, temporal_result::merge_temporal_query_results};
use andromeda_http_extraction::tenant_state;
use andromeda_http_response::content_range::build_content_range;
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::error::TemporalError;
use axum::http::HeaderMap;

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
    // considers and the sources it contacts; an expired or unknown one yields
    // no inference, so a fresh map is created instead.
    let mut entity_map_requested = params.entity_map.unwrap_or(false);
    let referenced_map = match &entity_map_header {
        Some(map_id) => match state.entity_maps.referenced(map_id.as_inner()).await.with_instance(&uri)? {
            ReferencedEntityMap::Live(map) => Some(*map),
            ReferencedEntityMap::Unavailable => {
                entity_map_requested = true;
                None
            }
        },
        None => None,
    };
    if let Some(map) = &referenced_map {
        let scoped_input = params.id.clone();
        params.id = Some(EntityMapService::scope_query_ids(map, scoped_input.as_deref()));

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
    let split_entities = state.forwarding.split_entities(params.split_entities) && !is_local;

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

    let result = service.query(&params, &context).await.with_instance(&uri)?;
    for warning in &result.warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal query warning");
    }
    let TemporalQueryResult {
        entities: local_entities,
        total_count,
        any_truncated,
        range,
        warnings: _,
        linked_candidates,
    } = result;

    // Distributed forwarding: query matching context sources unless local=true
    let mut remote_sources: Vec<(Uri, Vec<Uri>)> = Vec::new();
    let (mut entities, forwarding_warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let entity_ids_ref = entity_ids.as_deref();
        let query_string = uri.query().unwrap_or("");

        let forwarded = state
            .forwarding
            .forward_temporal_query(ForwardTemporalQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids_ref,
                query_string,
                context: &context,
                operation: ForwardingOperation::QueryTemporalEntity,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
                mode_filter: CsrModeFilter::All,
                entity_map: referenced_map.as_ref(),
            })
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        // Capture which registration returned which entity before the merge,
        // for the `entityMap` member (clauses 5.2.39 and 5.5.14).
        remote_sources = forwarded
            .iter()
            .filter_map(|result| {
                let registration = result.registration_id.clone()?; // clone: the map owns the registration it attributes entities to
                let ids = result
                    .entities
                    .iter()
                    .filter_map(|value| value.get("id").and_then(Value::as_str).and_then(|raw| raw.parse::<Uri>().ok()))
                    .collect();
                Some((registration, ids))
            })
            .collect();

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

    // EntityMap creation (spec 5.5.14, 5.7.4.4): if entityMap=true was requested,
    // create a new EntityMap from all matching IDs (not just the current page).
    let entity_map_uri = if entity_map_requested {
        let mut local_ids = service.query_ids(&params, &context).await.with_instance(&uri)?;
        // Linked entities the join resolved belong to the unitary request too (clause 5.5.14).
        local_ids.extend(linked_ids);
        local_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        local_ids.dedup();
        let lifetime = params.entity_map_lifetime.as_ref();
        let mut map = state.entity_maps.local_map(local_ids, lifetime).with_instance(&uri)?;
        for (registration, ids) in remote_sources {
            map.add_remote_entities(&registration, ids);
        }
        state.entity_maps.store(&map).await.with_instance(&uri)?;
        Some(map.id)
    } else {
        referenced_map.map(|map| map.id)
    };

    let status = if entity_map_requested { StatusCode::CREATED } else { StatusCode::OK };

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
