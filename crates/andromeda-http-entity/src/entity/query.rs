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
    remote_sources_of,
    render_flat_join,
    start_rendering,
    with_linked_resolution_warnings,
};
use andromeda_entity_map::service::{EntityMapService, ReferencedEntityMap};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use andromeda_http_response::pagination::{PageExtent, PaginationStyle, append_pagination_links};
use andromeda_ngsi_ld_request::query::SharedQueryParams;
use axum::http::HeaderMap;
use std::iter::empty;

pub(crate) async fn query(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    headers: HeaderMap,
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

        // Clause 5.5.14: a referenced live EntityMap fixes the entities the
        // query considers and the sources it contacts; an expired or unknown
        // one yields no inference, so a fresh map is created instead.
        let mut entity_map_requested = query.entity_map.unwrap_or(false);
        let referenced_map = match &entity_map_header {
            Some(map_id) => match state.entity_maps.referenced(map_id.as_inner()).await.with_entity_instance(&uri)? {
                ReferencedEntityMap::Live(map) => Some(*map),
                ReferencedEntityMap::Unavailable => {
                    entity_map_requested = true;
                    None
                }
            },
            None => None,
        };
        if let Some(map) = &referenced_map {
            let scoped_input = query.id.clone();
            query.id = Some(EntityMapService::scope_query_ids(map, scoped_input.as_deref()));
        }

        let opts = query.parse_options();
        let is_local = query.is_local();
        let split_entities = state.forwarding.split_entities(query.split_entities) && !is_local;

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
        let mut remote_sources: Vec<(Uri, Vec<Uri>)> = Vec::new();
        let (mut entities, warnings) = if is_local {
            (local_entities, Vec::new())
        } else {
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
                    context: &context,
                    operation: ForwardingOperation::QueryEntity,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                    csf_expr: csf_expr.as_ref(),
                    mode_filter: CsrModeFilter::ExcludeAuxiliary,
                    entity_map: referenced_map.as_ref(),
                })
                .await
                .unwrap_or_default();
            let mut warnings = collect_warnings(&forwarded);

            // Capture which registration returned which entity before the
            // merge, for the `entityMap` member (clauses 5.2.39 and 5.5.14).
            remote_sources.extend(remote_sources_of(&forwarded));

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
                    context: &context,
                    operation: ForwardingOperation::QueryEntity,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                    csf_expr: csf_expr.as_ref(),
                    mode_filter: CsrModeFilter::AuxiliaryOnly,
                    entity_map: referenced_map.as_ref(),
                })
                .await
                .unwrap_or_default();
            if !aux_forwarded.is_empty() {
                warnings.extend(collect_warnings(&aux_forwarded));
                remote_sources.extend(remote_sources_of(&aux_forwarded));
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

        // Clause 5.7.2.4 with `entityMap=true`: the map covers every matching
        // entity, not only the page, with each entity attributed to the sources
        // that hold it (clause 5.2.39).
        let entity_map_uri = if entity_map_requested {
            let compiled = service.compile_query_public(&query, &context).await.with_entity_instance(&uri)?;
            let mut local_ids = service.query_ids(&compiled).await.with_entity_instance(&uri)?;
            // Linked entities the join resolved belong to the unitary request too (clause 5.5.14).
            if let Some(resolved) = &resolved {
                local_ids.extend(resolved.linked.keys().cloned());
            }
            local_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
            local_ids.dedup();
            let lifetime = query.entity_map_lifetime.as_ref();
            let mut map = state.entity_maps.local_map(local_ids, lifetime).with_entity_instance(&uri)?;
            for (registration, ids) in remote_sources {
                map.add_remote_entities(&registration, ids);
            }
            state.entity_maps.store(&map).await.with_entity_instance(&uri)?;
            Some(map.id)
        } else {
            // Echo the referenced live map back in the response header.
            referenced_map.map(|map| map.id)
        };

        // Clause 6.3.10 / 4.12: pagination Link headers over the local page. A
        // query that asked for a count knows its total and drives `next` from it;
        // one that did not carries no total (the broker does not count on every
        // query) and signals a following page by the page coming back full.
        let page_limit = i64::from(query.effective_limit());
        let page_offset = i64::from(query.effective_offset());
        let page_extent = match total_count {
            Some(count) => PageExtent::Total(count),
            None => PageExtent::PageFullness {
                returned: entities.len() as i64,
            },
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
            let mut response = response.into_response();
            append_pagination_links(
                &mut response,
                &uri,
                page_limit,
                page_offset,
                page_extent,
                PaginationStyle::OffsetPrevNext,
                accept.media_type(),
            );
            return Ok(response);
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
        let mut response = response.into_response();
        append_pagination_links(
            &mut response,
            &uri,
            page_limit,
            page_offset,
            page_extent,
            PaginationStyle::OffsetPrevNext,
            accept.media_type(),
        );
        Ok(response)
    })
    .await
}
