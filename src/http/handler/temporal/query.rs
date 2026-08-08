//! HTTP handling for temporal query.

use crate::http::handler::temporal::{
    AcceptFormatNoGeo,
    AndromedaError,
    ContextSpec,
    ContextUrl,
    ContextualExpander,
    CsrModeFilter,
    DeferredSplitFilters,
    ForwardTemporalQueryRequest,
    ForwardingMode,
    ForwardingOperation,
    FromStr,
    IntoResponse,
    NgsiLdCollectionResponse,
    NgsiLdError,
    NgsiLdProtocolHeaders,
    OriginalUri,
    PreferHeader,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    StatusCode,
    TemporalQueryParameters,
    TemporalQueryResult,
    TemporalService,
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
};

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    protocol
        .extensions()
        .apply_to_temporal_query(&mut params)
        .map_err(|error| AndromedaError::from(error).with_instance(&uri))?;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;
    let key_values = params.is_simplified();

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, query temporal entities from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_type = params.entity_type.as_ref().map(ToString::to_string);
        let entity_type = entity_type.as_deref();
        let entity_ids = params.id.clone();
        let limit = params.limit.unwrap_or(100) as i64;
        let offset = params.offset.unwrap_or(0) as i64;

        let (entities, total) = state
            .snapshots
            .query_snapshot_temporal_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
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
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Collection(values))
            .render()?;
        let response = NgsiLdCollectionResponse::ok(rendered)
            .with_snapshot_uri(snapshot_id)
            .with_total_count(total as usize);
        return Ok(response.into_response());
    }

    // EntityMap scoping (spec 5.5.14): if NGSILD-EntityMap header is present,
    // scope the query to only the entity IDs in the map.
    if let Some(ref map_id) = entity_map_header {
        let scoped_input = params.id.clone();
        let scoped = state
            .entity_maps
            .scope_query_ids(map_id.as_inner(), scoped_input.as_deref())
            .await
            .with_instance(&uri)?;
        params.id = Some(scoped);

        // EntityMap-scoped temporal queries may omit temporal bounds; default to
        // an "all history up to far future" window so scoped follow-up queries succeed.
        if params.timerel.is_none() && params.time_at.is_none() {
            params.timerel = Some(TimeRel::Before);
            params.time_at = Some(
                chrono::DateTime::parse_from_rfc3339("9999-12-31T23:59:59Z")
                    .map(|instant| instant.with_timezone(&chrono::Utc))
                    .map_err(|source| AndromedaError::temporal_validation(format!("Invalid entity-map temporal horizon: {source}")).with_instance(&uri))?,
            );
        }
    }

    let is_local = params.is_local();
    let entity_map_requested = params.entity_map.unwrap_or(false);
    let split_entities = params.split_entities.unwrap_or(false) && !is_local;

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
        content_range,
        warnings: _,
    } = result;

    // Distributed forwarding: query matching context sources unless local=true
    let mut remote_ids: Vec<String> = Vec::new();
    let (mut entities, forwarding_warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = params.entity_type.as_ref().map(ToString::to_string);
        let entity_type = entity_type.as_deref();
        let entity_ids = params.id.clone();
        let entity_ids_ref = entity_ids.as_deref();
        let query_string = uri.query().unwrap_or("");

        let forwarded = state
            .forwarding
            .forward_temporal_query(
                &ForwardTemporalQueryRequest {
                    entity_type_compact: entity_type,
                    entity_ids: entity_ids_ref,
                    query_string,
                    context_url,
                    context: &context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    incoming_via: incoming_via.as_ref(),
                },
                CsrModeFilter::All,
            )
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        // Capture remote entity IDs before merge for entityMap (spec 5.7.4.4, 5.5.14)
        remote_ids = forwarded
            .iter()
            .flat_map(|f| f.entities.iter())
            .filter_map(|v| v.get("id").and_then(Value::as_str).map(String::from))
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
        let merged = TemporalService::merge_with_forwarded(local_entities, forwarded);
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

    // EntityMap creation (spec 5.5.14, 5.7.4.4): if entityMap=true was requested,
    // create a new EntityMap from all matching IDs (not just the current page).
    let entity_map_uri = if entity_map_requested {
        let mut all_ids = service.query_ids(&params, &context).await.with_instance(&uri)?;
        // Include forwarded entity IDs in the map (spec 5.7.4.4, 5.5.14)
        all_ids.extend(remote_ids);
        all_ids.sort_unstable();
        all_ids.dedup();
        let typed_ids: Vec<Uri> = all_ids
            .iter()
            .map(|id| Uri::from_str(id).map_err(|_| AndromedaError::temporal_validation(format!("Invalid entity id URI '{}'", id))))
            .collect::<Result<Vec<_>, _>>()
            .with_instance(&uri)?;
        let lifetime = params.entity_map_lifetime.as_ref();
        let map = state.entity_maps.create(typed_ids, lifetime).await.with_instance(&uri)?;
        Some(map.id)
    } else if entity_map_header.is_some() {
        entity_map_header.map(|h| h.into_inner())
    } else {
        None
    };

    let status = if entity_map_requested { StatusCode::CREATED } else { StatusCode::OK };
    let returned = entities.len();

    let values = if key_values {
        TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Collection(entities))
            .render()
            .with_instance(&uri)?
            .into_collection()
            .with_instance(&uri)?
    } else {
        entities
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Collection(values))
        .render()?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(forwarding_warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
        if (count as usize) > returned {
            response = response.with_status(StatusCode::PARTIAL_CONTENT);
        }
    }
    if any_truncated {
        response = response.with_status(StatusCode::PARTIAL_CONTENT);
    }
    if let Some(cr) = content_range {
        response = response.with_content_range(cr);
    }
    if let Some(ref map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri.clone()); // clone: owned String needed for response header; map_uri borrowed by ref pattern
    }
    Ok(response.into_response())
}
