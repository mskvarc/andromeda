//! HTTP handling for the query entity operation.

use crate::entity::{
    AcceptFormat,
    ContextSpec,
    ContextUrl,
    ContextualExpander,
    CsrModeFilter,
    DeferredSplitFilters,
    EntityQuery,
    EntityQueryService,
    EntityRenderer,
    EntityResultExt,
    EntityTenantState,
    ForwardQueryRequest,
    ForwardingOperation,
    IntoResponse,
    JoinRequest,
    NgsiLdCollectionResponse,
    NgsiLdProtocolHeaders,
    OriginalUri,
    ProblemResponse,
    RequestProjection,
    ResolvedJoin,
    Response,
    ResponsePayload,
    RetrievingJoin,
    SnapshotRenderer,
    StatusCode,
    Uri,
    Value,
    collect_warnings,
    compile_geo_query,
    evaluate,
    expand_q_expr,
    matches_geo_query,
    matches_scope_query,
    render_flat_join,
    start_rendering,
    with_linked_resolution_warnings,
};
use andromeda_http_extraction::tenant_state;
use std::iter::empty;

pub(crate) async fn query(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    EntityQuery(mut query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    Box::pin(async move {
        let service = &state.entity_query;
        let context: ContextSpec = context.into_inner();
        protocol.extensions().apply_to_entity_query(&mut query).with_entity_instance(&uri)?;
        query.validate().with_entity_instance(&uri)?;
        let language_filter = query.lang.take().or(protocol.language_filter);
        let prefer = protocol.prefer;
        let snapshot = protocol.snapshot;
        let incoming_via = protocol.incoming_via;
        let entity_map_header = protocol.entity_map;

        // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
        // is present, query entities from the snapshot's frozen data and return early.
        if let Some(snapshot_id) = snapshot.into_inner() {
            state.snapshots.validate(&snapshot_id).await.with_entity_instance(&uri)?;

            let entity_type = query.entity_type.as_ref();
            let entity_ids = query.id.clone();
            let limit = query.limit.unwrap_or(100) as i64;
            let offset = query.offset.unwrap_or(0) as i64;

            let (entities, total) = state
                .snapshots
                .entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
                .await
                .with_entity_instance(&uri)?;

            let opts = query.parse_options();
            let filtered: Vec<Value> = entities
                .into_iter()
                .map(|v| {
                    SnapshotRenderer::new(v)
                        .with_attrs_filter(query.attrs.as_deref())
                        .with_mode(opts.render_mode)
                        .render()
                })
                .collect::<Result<Vec<_>, _>>()
                .with_entity_instance(&uri)?;

            let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
                .with_geometry_property(query.geometry_property.as_ref())
                .with_payload(ResponsePayload::Collection(filtered))
                .render()
                .with_entity_instance(&uri)?;
            let response = NgsiLdCollectionResponse::ok(rendered)
                .with_snapshot_uri(snapshot_id)
                .with_total_count(total as usize);

            return Ok(response.into_response());
        }

        // §5.5.9.3: If NGSILD-EntityMap header present, scope query to that map's entity IDs
        if let Some(ref map_id) = entity_map_header {
            let scoped_input = query.id.clone();
            let scoped = state
                .entity_maps
                .scope_query_ids(map_id.as_inner(), scoped_input.as_deref())
                .await
                .with_entity_instance(&uri)?;
            query.id = Some(scoped);
        }

        let opts = query.parse_options();
        let is_local = query.local.unwrap_or(false);
        let entity_map_requested = query.entity_map.unwrap_or(false);
        let split_entities = query.split_entities.unwrap_or(false) && !is_local;

        // splitEntities (spec 5.7.2.4): defer q/geoQ/scopeQ filtering to post-merge
        let deferred = if split_entities {
            DeferredSplitFilters {
                q: query.q.take(),
                geo: query.geo.0.take(),
                scope_q: query.scope_q.take(),
            }
        } else {
            DeferredSplitFilters::default()
        };

        let (local_entities, total_count) = service.query(&query, &context).await.with_entity_instance(&uri)?;

        // Distributed forwarding: query matching context sources unless local=true
        let mut remote_ids: Vec<Uri> = Vec::new();
        let (mut entities, warnings) = if is_local {
            (local_entities, Vec::new())
        } else {
            let context_url = context.link_contexts().first();
            let entity_type = query.entity_type.as_ref();
            let entity_ids = query.id.clone();
            let entity_ids_ref = entity_ids.as_deref();

            // The CSR filter was parsed while extracting the request.
            let prepared_context = state.entity_query.engine().prepare_term_context(&context);
            let csf_expander = ContextualExpander::new(state.entity_query.engine(), &prepared_context);
            let csf_expr = match query.csf.take() {
                Some(expr) => Some(expand_q_expr(expr, &csf_expander).await.with_entity_instance(&uri)?),
                None => None,
            };

            // Phase 1: forward to primary (non-auxiliary) CSRs
            let forwarded = state
                .forwarding
                .forward_query(ForwardQueryRequest {
                    entity_type_compact: entity_type,
                    entity_ids: entity_ids_ref,
                    context_url,
                    context: &context,
                    operation: ForwardingOperation::QueryEntity,
                    incoming_via: incoming_via.as_ref(),
                    csf_expr: csf_expr.as_ref(),
                    mode_filter: CsrModeFilter::ExcludeAuxiliary,
                })
                .await
                .unwrap_or_default();
            let mut warnings = collect_warnings(&forwarded);

            // Capture remote entity IDs before merge for entityMap (spec 5.7.2.4, 5.5.14)
            remote_ids = forwarded
                .iter()
                .flat_map(|result| result.entities.iter())
                .map(|entity| entity.id.clone())
                .collect();

            let merged = if forwarded.is_empty() {
                local_entities
            } else {
                EntityQueryService::merge_with_forwarded(local_entities, forwarded, query.attrs.as_deref())
            };

            // Phase 2: forward to auxiliary CSRs and merge their entities/attributes.
            let aux_forwarded = state
                .forwarding
                .forward_query(ForwardQueryRequest {
                    entity_type_compact: entity_type,
                    entity_ids: entity_ids_ref,
                    context_url,
                    context: &context,
                    operation: ForwardingOperation::QueryEntity,
                    incoming_via: incoming_via.as_ref(),
                    csf_expr: csf_expr.as_ref(),
                    mode_filter: CsrModeFilter::AuxiliaryOnly,
                })
                .await
                .unwrap_or_default();
            if !aux_forwarded.is_empty() {
                warnings.extend(collect_warnings(&aux_forwarded));
                let aux_remote_ids: Vec<Uri> = aux_forwarded
                    .iter()
                    .flat_map(|result| result.entities.iter())
                    .map(|entity| entity.id.clone())
                    .collect();
                remote_ids.extend(aux_remote_ids);
                let merged = EntityQueryService::merge_with_forwarded(merged, aux_forwarded, query.attrs.as_deref());
                (merged, warnings)
            } else {
                (merged, warnings)
            }
        };

        // splitEntities post-merge filtering: apply deferred q/geoQ/scopeQ (spec 5.7.2.4)
        if split_entities {
            let q_expr = if let Some(q_expr) = deferred.q {
                let prepared_context = service.engine().prepare_term_context(&context);
                let expander = ContextualExpander::new(service.engine(), &prepared_context);
                Some(expand_q_expr(q_expr, &expander).await.with_entity_instance(&uri)?)
            } else {
                None
            };
            let prepared_geo_context = service.engine().prepare_term_context(&context);
            let geo_expander = ContextualExpander::new(service.engine(), &prepared_geo_context);
            let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &geo_expander).await.with_entity_instance(&uri)?;
            let compiled_scope = deferred.scope_q;

            let mut filtered = Vec::with_capacity(entities.len());
            for entity in entities {
                let compact = match serde_json::to_value(&entity) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let expanded = match service.engine().expand(compact, &context).await {
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

        // Join mode (clause 4.5.23) is parsed at the query boundary. `@none`
        // and an absent `join` retrieve no linked entity at all, so neither
        // reaches the traversal. The join resolves before EntityMap creation
        // so a requested map can include the linked ids (spec 5.5.14, 5.7.2.4).
        // Clause 4.21: the request's `pick`/`omit` govern both the matching
        // entities' structural members and, through their clause 4.21 nested
        // terms, the entities the traversal reaches. Both read it off the
        // compilation the query already ran, which has to outlive the join
        // that borrows its terms.
        let request_projection = RequestProjection::of(service, &query, &context).await.with_entity_instance(&uri)?;
        let projection = request_projection.projection();

        let join = RetrievingJoin::from_mode(query.join);
        let resolved = match join {
            Some(join) => {
                let request = JoinRequest {
                    join,
                    join_level: query.join_level,
                    contained_by: query.contained_by.as_deref(),
                    sys_attrs: opts.sys_attrs,
                    projection,
                };
                Some(ResolvedJoin::prepare(service, &entities, &context, &request).await.with_entity_instance(&uri)?)
            }
            None => None,
        };
        // Clause 6.3.17: a Linked Entity the join could not reach leaves the
        // body incomplete, and the warning header is what says so.
        let warnings = match &resolved {
            Some(resolved) => with_linked_resolution_warnings(warnings, &resolved.failures),
            None => warnings,
        };

        // §5.7.2: If entityMap=true, create an EntityMap and set header
        let entity_map_uri = if entity_map_requested {
            let compiled = service.compile_query_public(&query, &context).await.with_entity_instance(&uri)?;
            let mut all_ids = service.query_ids(&compiled).await.with_entity_instance(&uri)?;
            // Include forwarded entity IDs in the map (spec 5.7.2.4, 5.5.14)
            all_ids.extend(remote_ids);
            // Include linked entity IDs the join resolved (spec 5.5.14, 5.7.2.4)
            if let Some(resolved) = &resolved {
                all_ids.extend(resolved.linked.keys().cloned());
            }
            all_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
            all_ids.dedup();
            let lifetime = query.entity_map_lifetime.as_ref();
            let map = state.entity_maps.create(all_ids, lifetime).await.with_entity_instance(&uri)?;
            Some(map.id)
        } else if entity_map_header.is_some() {
            // Echo the same map back in the response header
            entity_map_header.map(|h| h.into_inner())
        } else {
            None
        };

        let status = if entity_map_requested { StatusCode::CREATED } else { StatusCode::OK };

        if let (Some(join), Some(resolved)) = (join, resolved) {
            let payload = match join {
                // Flat: the root entities and their linked entities side by
                // side, the roots ahead of the linked (clause 4.5.23).
                RetrievingJoin::Flat => render_flat_join(
                    entities.iter(),
                    resolved.linked.values(),
                    language_filter.as_ref(),
                    opts.render_mode,
                    projection,
                )
                .with_entity_instance(&uri)?,
                // Inline: the renderer nests the linked entities into each
                // root, narrowing each nested copy by the sub-projection its
                // own Relationship carried (clause 4.21).
                RetrievingJoin::Inline => {
                    let mut entity_values: Vec<Value> = Vec::with_capacity(entities.len());
                    for entity in &entities {
                        entity_values.push(
                            EntityRenderer::new(entity)
                                .with_optional_language_filter(language_filter.as_ref())
                                .with_inline_linked_entities(&resolved.linked)
                                .with_optional_linked_projection(resolved.linked_projection())
                                .with_optional_structural_projection(projection)
                                .with_mode(opts.render_mode)
                                .render()
                                .with_entity_instance(&uri)?,
                        );
                    }
                    entity_values
                }
            };

            let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
                .with_geometry_property(query.geometry_property.as_ref())
                .with_payload(ResponsePayload::Collection(payload))
                .render()
                .with_entity_instance(&uri)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            if let Some(ref map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri.clone());
            }
            return Ok(response.into_response());
        }

        let rendered = render_flat_join(entities.iter(), empty(), language_filter.as_ref(), opts.render_mode, projection).with_entity_instance(&uri)?;
        let rendered_output = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
            .with_geometry_property(query.geometry_property.as_ref())
            .with_payload(ResponsePayload::Collection(rendered))
            .render()
            .with_entity_instance(&uri)?;
        let mut response = NgsiLdCollectionResponse::ok(rendered_output).with_status(status).with_warnings(warnings);
        if let Some(count) = total_count {
            response = response.with_total_count(count as usize);
        }
        if let Some(ref map_uri) = entity_map_uri {
            response = response.with_entity_map_uri(map_uri.clone());
        }
        Ok(response.into_response())
    })
    .await
}
