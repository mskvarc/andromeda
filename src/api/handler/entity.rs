use crate::{
    api::{
        handler::parse::{parse_entity_id, validate_attr_name},
        tenant::{TenantState, TenantStateOrCreate},
    },
    domain::entity::{Entity, EntityPatch, EntityTypeValue},
    error::AndromedaError,
    http::{
        error::{
            batch::{BatchEntityError, BatchOperationResult},
            problem::NgsiLdError,
        },
        extractor::{
            accept::AcceptFormat,
            context::ContextUrl,
            entity_query::EntityQuery,
            jsonld::JsonLd,
            protocol::NgsiLdProtocolHeaders,
            query::ValidatedQuery,
            via::IncomingVia,
        },
        parameter::{
            DeleteAttrParams,
            LocalOnlyParams,
            entity::{AppendParams, GetEntityParams, PartialUpdateAttrParams},
            protocol::{apply_entity_get_extension_headers, apply_entity_query_extension_headers},
        },
        response::{collection::NgsiLdCollectionResponse, single::NgsiLdResponse},
    },
    jsonld::ContextSpec,
    query::{compile::expand_q_expression, q::eval::evaluate},
    renderer::{
        SysAttrsInclusion,
        entity::{EntityRenderer, render_snapshot},
        response::ResponseRenderer,
    },
    service::{
        context_source_forwarding::{
            ForwardBatchProvisionRequest,
            ForwardProvisionRequest,
            ForwardingOperation,
            collect_forwarded_exclusive_attrs,
            collect_warnings,
            merge_retrieve_results,
            strip_exclusive_attrs_from_value,
        },
        entity::{
            provision_forwarding::{ForwardingModes, build_multi_status_body, first_forwarded_status, has_remote_errors},
            query::EntityQueryService,
        },
    },
    worker::notification::matching::{matches_geo_filter, matches_scope_filter},
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::collections::HashSet;

pub async fn patch_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    Err(NgsiLdError::from(AndromedaError::bad_request_data("Entity id is required").with_instance(uri)))
}

pub async fn put_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    Err(NgsiLdError::from(AndromedaError::bad_request_data("Entity id is required").with_instance(uri)))
}

pub async fn create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: entity,
        context: body_context,
    }: JsonLd<Entity>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);

    if is_local {
        let id = service.create(&entity, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response());
    }

    // Check for matching CSRs for distributed forwarding
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_value =
        serde_json::to_value(&entity).map_err(|e| AndromedaError::internal("Failed to serialize entity").with_source(e).with_instance(uri.clone()))?;
    let entity_body =
        serde_json::to_vec(&entity_value).map_err(|e| AndromedaError::internal("Failed to serialize entity").with_source(e).with_instance(uri.clone()))?;
    let entity_id = entity.id.to_string();
    let entity_type_str = match &entity.type_ {
        EntityTypeValue::Single(s) => s.clone(),
        EntityTypeValue::Multiple(v) => v.first().cloned().unwrap_or_default(),
    };

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: Some(&entity_type_str),
            method: reqwest::Method::POST,
            body: Some(&entity_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::CreateEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        let id = service.create(&entity, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 201)).unwrap_or(StatusCode::CREATED);
        if status == StatusCode::CREATED {
            return Ok((status, [("Location", format!("/ngsi-ld/v1/entities/{}", entity_id))]).into_response());
        }
        return Ok(status.into_response());
    }

    if modes.has_inclusive {
        let local_result = service.create(&entity, &context).await;
        let local_ok = local_result.is_ok();
        let local_error = local_result.err();

        if local_ok && !has_remote_errors(&forwarded) {
            return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", entity_id))]).into_response());
        }

        let body = build_multi_status_body(&entity_id, local_ok, local_error.as_ref(), &forwarded);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let id = service.create(&entity, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_query;
    let id = parse_entity_id(&id, &uri)?;
    let context: ContextSpec = context.into_inner();
    apply_entity_get_extension_headers(&mut params, &protocol, &uri)?;
    let language_filter = protocol.language_filter;
    let prefer = protocol.prefer;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;

    // Validate options before entity lookup so invalid options return 400, not 404
    let options = params.options.as_deref().unwrap_or("");
    for opt in options.split(',').map(str::trim) {
        if !opt.is_empty() && !matches!(opt, "keyValues" | "simplified" | "sysAttrs" | "concise" | "normalized") {
            return Err(NgsiLdError::from(
                AndromedaError::invalid_request(format!("Invalid options value: '{}'", opt)).with_instance(uri.clone()),
            ));
        }
    }

    // Validate format parameter
    if let Some(ref fmt) = params.format
        && !matches!(fmt.as_str(), "normalized" | "keyValues" | "simplified" | "concise")
    {
        return Err(NgsiLdError::from(
            AndromedaError::invalid_request(format!("Invalid format value: '{}'", fmt)).with_instance(uri.clone()),
        ));
    }

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state
            .snapshots
            .validate_for_scoping(&snapshot_id)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        let entity_data = state
            .snapshots
            .get_snapshot_entity(&snapshot_id, id.as_ref())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?
            .ok_or_else(|| NgsiLdError::from(AndromedaError::not_found(id.to_string()).with_instance(uri.clone())))?;

        let mode = params.resolve_render_mode();
        let attrs: Option<Vec<&str>> = params.attrs.as_deref().map(|s| s.split(',').map(str::trim).collect());
        let result = render_snapshot(entity_data, attrs.as_deref(), mode);

        let rendered = ResponseRenderer::new(accept.format(), context, prefer)
            .with_geometry_property(params.geometry_property.clone())
            .render_single(result)?;
        return Ok(NgsiLdResponse::ok(rendered).with_snapshot_uri(snapshot_id).into_response());
    }

    let attrs: Vec<String> = params
        .attrs
        .as_deref()
        .map(|s| s.split(',').map(str::trim).map(String::from).collect())
        .unwrap_or_default();

    let sys_attrs = if options.contains("sysAttrs") {
        SysAttrsInclusion::Include
    } else {
        SysAttrsInclusion::Strip
    };
    let dataset_ids: Vec<String> = params
        .dataset_id
        .as_deref()
        .map(|s| s.split(',').map(str::trim).map(String::from).collect())
        .unwrap_or_default();
    let local_result = service.get_inner(&id, &attrs, sys_attrs, &dataset_ids, &context).await;

    let is_local = params.local.unwrap_or(false);

    // If local=true, skip distributed forwarding
    let (entity, warnings) = if is_local {
        let entity = local_result.map_err(|e| e.with_instance(uri.clone()))?;
        (entity, Vec::new())
    } else {
        // Forward to context sources
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_retrieve(
                id.as_ref(),
                query_string,
                context_url,
                ForwardingOperation::RetrieveEntity,
                incoming_via.as_ref(),
            )
            .await
            .unwrap_or_default(); // Retrieve forwarding: best-effort -- missing CSR data is not fatal
        let warnings = collect_warnings(&forwarded);

        let local_value = match local_result {
            Ok(e) => Some(serde_json::to_value(&e).map_err(|err| {
                AndromedaError::internal("Failed to serialize local entity")
                    .with_source(err)
                    .with_instance(uri.clone())
            })?),
            Err(_) => None,
        };

        let merged = merge_retrieve_results(local_value, forwarded);
        let entity = match merged {
            Some(value) => serde_json::from_value::<Entity>(value).map_err(|e| {
                AndromedaError::internal(format!("Failed to deserialize merged entity {}", id))
                    .with_source(e)
                    .with_instance(uri.clone())
            })?,
            None => {
                return Err(NgsiLdError::from(AndromedaError::not_found(id.to_string()).with_instance(uri.clone())));
            }
        };
        (entity, warnings)
    };

    let mode = params.resolve_render_mode();

    // Handle join parameter
    let join_mode = params.join.as_deref();
    let join_level = params.join_level.unwrap_or(1);

    let contained_by: HashSet<String> = params
        .contained_by
        .as_deref()
        .map(|s| s.split(',').map(|id| id.trim().to_string()).collect())
        .unwrap_or_default();

    if matches!(join_mode, Some("inline") | Some("flat")) {
        let linked = service
            .resolve_linked_entities(std::slice::from_ref(&entity), join_level, sys_attrs, &context, &contained_by)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        if join_mode == Some("flat") {
            // Flat: always return array of [root, ...linked] per NGSI-LD spec
            let mut all_entities: Vec<Value> = Vec::new();
            all_entities.push(
                EntityRenderer::new(&entity)
                    .with_optional_language_filter(language_filter.as_ref())
                    .with_mode(mode)
                    .render()
                    .map_err(|e| e.with_instance(uri.clone()))?,
            );
            for linked_entity in linked.values() {
                all_entities.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, prefer).render_collection(all_entities)?;
            return Ok(NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings).into_response());
        } else {
            // Inline: renderer handles join + mode transformation
            let entity_value = EntityRenderer::new(&entity)
                .with_optional_language_filter(language_filter.as_ref())
                .with_inline_linked_entities(&linked)
                .with_mode(mode)
                .render()
                .map_err(|e| e.with_instance(uri.clone()))?;
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_geometry_property(params.geometry_property.clone())
                .render_single(entity_value)?;
            return Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response());
        }
    }

    let entity_value = EntityRenderer::new(&entity)
        .with_optional_language_filter(language_filter.as_ref())
        .with_mode(mode)
        .render()
        .map_err(|e| e.with_instance(uri.clone()))?;
    let rendered = ResponseRenderer::new(accept.format(), context, prefer)
        .with_geometry_property(params.geometry_property.clone())
        .render_single(entity_value)?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}

pub async fn delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let is_local = params.local.unwrap_or(false);

    if is_local {
        service.delete(&id).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context: ContextSpec = context.into_inner();
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_id = id.to_string();

    let (forwarded, unforwardable_exclusive) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::DELETE,
            body: None,
            context_url,
            context: &context,
            operation: ForwardingOperation::DeleteEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    // Per spec 4.3.6.3: if an exclusive CSR exists but doesn't support this operation,
    // delete locally but return 207 to indicate exclusive data could not be cleaned up.
    if forwarded.is_empty() && !unforwardable_exclusive.is_empty() {
        let local_result = service.delete(&id).await;
        let body = build_multi_status_body(&entity_id, local_result.is_ok(), local_result.err().as_ref(), &[]);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    if forwarded.is_empty() {
        service.delete(&id).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive {
        if let Err(e) = service.delete(&id).await {
            tracing::warn!(error = %e, entity_id = %id, "Failed to delete local entity during exclusive forwarding");
        }
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    if modes.has_redirect {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    if modes.has_inclusive {
        let local_result = service.delete(&id).await;
        let local_ok = local_result.is_ok();
        let local_error = local_result.err();

        if local_ok && !has_remote_errors(&forwarded) {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }

        let body = build_multi_status_body(&entity_id, local_ok, local_error.as_ref(), &forwarded);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    service.delete(&id).await.map_err(|e| e.with_instance(uri.clone()))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    EntityQuery(mut query): EntityQuery,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_query;
    let context: ContextSpec = context.into_inner();
    apply_entity_query_extension_headers(&mut query, &protocol, &uri)?;
    query
        .validate()
        .map_err(|error| NgsiLdError::from(AndromedaError::from(error).with_instance(uri.clone())))?;
    let language_filter = protocol.language_filter;
    let prefer = protocol.prefer;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, query entities from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state
            .snapshots
            .validate_for_scoping(&snapshot_id)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        let entity_type = query.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = query.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let limit = query.limit.unwrap_or(100) as i64;
        let offset = query.offset.unwrap_or(0) as i64;

        let (entities, total) = state
            .snapshots
            .query_snapshot_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        let opts = query.parse_options();
        let attrs: Option<Vec<&str>> = query.attrs.as_deref().map(|s| s.split(',').map(str::trim).collect());
        let filtered: Vec<Value> = entities.into_iter().map(|v| render_snapshot(v, attrs.as_deref(), opts.render_mode)).collect();

        let rendered = ResponseRenderer::new(accept.format(), context, prefer)
            .with_geometry_property(query.geometry_property.clone())
            .render_collection(filtered)?;
        let response = NgsiLdCollectionResponse::ok(rendered)
            .with_snapshot_uri(snapshot_id)
            .with_total_count(total as usize);

        return Ok(response.into_response());
    }

    // §5.5.9.3: If NGSILD-EntityMap header present, scope query to that map's entity IDs
    if let Some(ref map_id) = entity_map_header {
        let scoped = state
            .entity_maps
            .scope_query_ids(map_id.as_str(), query.id.as_deref())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;
        query.id = Some(scoped);
    }

    let opts = query.parse_options();
    let is_local = query.local.unwrap_or(false);
    let entity_map_requested = query.entity_map.unwrap_or(false);
    let split_entities = query.split_entities.unwrap_or(false) && !is_local;

    // splitEntities (spec 5.7.2.4): defer q/geoQ/scopeQ filtering to post-merge
    let deferred_q;
    let deferred_georel;
    let deferred_geometry;
    let deferred_coordinates;
    let deferred_geoproperty;
    let deferred_scope_q;
    if split_entities {
        deferred_q = query.q.take();
        deferred_georel = query.georel.take();
        deferred_geometry = query.geometry.take();
        deferred_coordinates = query.coordinates.take();
        deferred_geoproperty = query.geoproperty.take();
        deferred_scope_q = query.scope_q.take();
    } else {
        deferred_q = None;
        deferred_georel = None;
        deferred_geometry = None;
        deferred_coordinates = None;
        deferred_geoproperty = None;
        deferred_scope_q = None;
    }

    let (local_entities, total_count) = service.query(&query, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    // Distributed forwarding: query matching context sources unless local=true
    let (mut entities, warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = query.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = query.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_query(
                entity_type,
                entity_ids_ref,
                context_url,
                &context,
                ForwardingOperation::QueryEntity,
                incoming_via.as_ref(),
            )
            .await
            .unwrap_or_default();
        let warnings = collect_warnings(&forwarded);

        if forwarded.is_empty() {
            (local_entities, warnings)
        } else {
            let merged = EntityQueryService::merge_with_forwarded(local_entities, forwarded, query.attrs.as_deref());
            (merged, warnings)
        }
    };

    // splitEntities post-merge filtering: apply deferred q/geoQ/scopeQ (spec 5.7.2.4)
    if split_entities {
        if let Some(ref q_str) = deferred_q
            && let Ok(Some(q_expr)) = expand_q_expression(Some(q_str), service.engine(), &context).await
        {
            entities.retain(|e| {
                let val = serde_json::to_value(e).unwrap_or_default();
                evaluate(&q_expr, &val)
            });
        }
        if let (Some(georel), Some(geometry), Some(coordinates)) = (&deferred_georel, &deferred_geometry, &deferred_coordinates) {
            let mut geo_val = serde_json::json!({
                "georel": georel,
                "geometry": geometry,
                "coordinates": coordinates,
            });
            if let Some(ref geoprop) = deferred_geoproperty {
                geo_val["geoproperty"] = Value::String(geoprop.clone());
            }
            entities.retain(|e| {
                let val = serde_json::to_value(e).unwrap_or_default();
                matches_geo_filter(&geo_val, &val)
            });
        }
        if let Some(ref scope_q) = deferred_scope_q {
            entities.retain(|e| {
                let val = serde_json::to_value(e).unwrap_or_default();
                matches_scope_filter(scope_q, &val)
            });
        }
    }

    // §5.7.2: If entityMap=true, create an EntityMap and set header
    let entity_map_uri = if entity_map_requested {
        let compiled = service.compile_query_public(&query, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        let all_ids = service.query_ids(&compiled).await.map_err(|e| e.with_instance(uri.clone()))?;
        let map = state
            .entity_maps
            .create(all_ids, query.entity_map_lifetime.as_deref())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;
        Some(map.id)
    } else if entity_map_header.is_some() {
        // Echo the same map back in the response header
        entity_map_header.map(|h| h.into_inner().to_string())
    } else {
        None
    };

    let status = if entity_map_requested { StatusCode::CREATED } else { StatusCode::OK };

    // Handle join
    let join_mode = query.join.as_deref();
    let join_level = query.join_level.unwrap_or(1);
    let contained_by: HashSet<String> = query
        .contained_by
        .as_deref()
        .map(|s| s.split(',').map(|id| id.trim().to_string()).collect())
        .unwrap_or_default();

    if matches!(join_mode, Some("inline") | Some("flat")) {
        let linked = service
            .resolve_linked_entities(&entities, join_level, opts.sys_attrs, &context, &contained_by)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        if join_mode == Some("flat") {
            let mut all_values: Vec<Value> = Vec::new();
            for entity in &entities {
                all_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            for linked_entity in linked.values() {
                all_values.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_geometry_property(query.geometry_property.clone())
                .render_collection(all_values)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            if let Some(ref map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri.clone());
            }
            return Ok(response.into_response());
        } else {
            // Inline: renderer handles join + mode transformation
            let mut entity_values: Vec<Value> = Vec::new();
            for entity in &entities {
                entity_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_inline_linked_entities(&linked)
                        .with_mode(opts.render_mode)
                        .render()
                        .map_err(|e| e.with_instance(uri.clone()))?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_geometry_property(query.geometry_property.clone())
                .render_collection(entity_values)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            if let Some(ref map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri.clone());
            }
            return Ok(response.into_response());
        }
    }

    let rendered = entities
        .iter()
        .map(|entity| {
            EntityRenderer::new(entity)
                .with_optional_language_filter(language_filter.as_ref())
                .with_mode(opts.render_mode)
                .render()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.with_instance(uri.clone()))?;
    let rendered_output = ResponseRenderer::new(accept.format(), context, prefer)
        .with_geometry_property(query.geometry_property.clone())
        .render_collection(rendered)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered_output).with_status(status).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    if let Some(ref map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri.clone());
    }
    Ok(response.into_response())
}

pub async fn merge(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: patch,
        context: body_context,
    }: JsonLd<EntityPatch>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);

    if is_local {
        service.merge(&id, &patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let patch_value =
        serde_json::to_value(&patch).map_err(|e| AndromedaError::internal("Failed to serialize patch").with_source(e).with_instance(uri.clone()))?;
    let patch_body =
        serde_json::to_vec(&patch_value).map_err(|e| AndromedaError::internal("Failed to serialize patch").with_source(e).with_instance(uri.clone()))?;
    let entity_id = id.to_string();

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::PATCH,
            body: Some(&patch_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::MergeEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        service.merge(&id, &patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    if !exclusive_attrs.is_empty() {
        let mut filtered_patch = patch.clone(); // clone: need modified copy; original consumed by forwarding
        if !filtered_patch.strip_exclusive_attrs(&exclusive_attrs) {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        service.merge(&id, &filtered_patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    } else {
        service.merge(&id, &patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: patch,
        context: body_context,
    }: JsonLd<EntityPatch>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);

    if is_local {
        service.replace_entity(&id, &patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let patch_value =
        serde_json::to_value(&patch).map_err(|e| AndromedaError::internal("Failed to serialize patch").with_source(e).with_instance(uri.clone()))?;
    let patch_body =
        serde_json::to_vec(&patch_value).map_err(|e| AndromedaError::internal("Failed to serialize patch").with_source(e).with_instance(uri.clone()))?;
    let entity_id = id.to_string();

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::PUT,
            body: Some(&patch_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::ReplaceEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        service.replace_entity(&id, &patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    if !exclusive_attrs.is_empty() {
        let mut filtered_patch = patch.clone(); // clone: need modified copy; original consumed by forwarding
        filtered_patch.strip_exclusive_attrs(&exclusive_attrs);
        service
            .replace_entity(&id, &filtered_patch, &context)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;
    } else {
        service.replace_entity(&id, &patch, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn append_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<AppendParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    let no_overwrite = params.options.as_deref().is_some_and(|o: &str| o.contains("noOverwrite"));

    // Forward to matching context sources
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let fragment_body = serde_json::to_vec(&fragment).map_err(|e| {
        AndromedaError::internal("Failed to serialize fragment")
            .with_source(e)
            .with_instance(uri.clone())
    })?;
    let entity_id = id.to_string();
    let qs = if no_overwrite { "options=noOverwrite" } else { "" };

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::POST,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::AppendEntityAttributes,
            path_suffix: Some("/attrs"),
            query_string: if qs.is_empty() { None } else { Some(qs) },
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        // No matching CSRs — append locally
        let (all_attrs, not_appended) = service
            .append_attrs(&id, fragment, no_overwrite, &context)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        if not_appended.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let updated: Vec<&String> = all_attrs.iter().filter(|a| !not_appended.contains(a)).collect();
        let not_updated_details: Vec<serde_json::Value> = not_appended
            .iter()
            .map(|attr| serde_json::json!({ "attributeName": attr, "reason": "Already exists" }))
            .collect();
        let body = serde_json::json!({ "updated": updated, "notUpdated": not_updated_details });
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let local_fragment = if !exclusive_attrs.is_empty() {
        let mut filtered = fragment.clone(); // clone: need modified copy; original consumed by forwarding
        strip_exclusive_attrs_from_value(&mut filtered, &exclusive_attrs);
        filtered
    } else {
        fragment
    };

    // Apply locally (ignore errors for inclusive -- remote already succeeded)
    let local_result = service.append_attrs(&id, local_fragment, no_overwrite, &context).await;
    let remote_errors = has_remote_errors(&forwarded);

    if let Ok((_, not_appended)) = &local_result
        && !remote_errors
        && not_appended.is_empty()
    {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // 207 Multi-Status for mixed results
    let mut errors = Vec::new();
    if let Err(err) = local_result {
        errors.push(serde_json::to_value(BatchEntityError::from_domain_error(&entity_id, &err)).unwrap_or_default());
    }
    for r in &forwarded {
        if r.status >= 400 {
            let error_body = r
                .body
                .clone()
                .unwrap_or_else(|| serde_json::json!({ "status": r.status, "title": "Error from context source" }));
            errors.push(serde_json::json!({ "entityId": entity_id, "error": error_body }));
        }
    }
    let body = serde_json::json!({ "success": [entity_id], "errors": errors });
    Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
}

pub async fn update_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);

    // Forward to matching context sources
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let fragment_body = serde_json::to_vec(&fragment).map_err(|e| {
        AndromedaError::internal("Failed to serialize fragment")
            .with_source(e)
            .with_instance(uri.clone())
    })?;
    let entity_id = id.to_string();

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::PATCH,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::UpdateEntityAttributes,
            path_suffix: Some("/attrs"),
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        let not_updated = service.update_attrs(&id, fragment, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        if not_updated.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let not_updated_details: Vec<serde_json::Value> = not_updated
            .iter()
            .map(|attr| serde_json::json!({ "attributeName": attr, "reason": "Attribute not found" }))
            .collect();
        let body = serde_json::json!({ "updated": [], "notUpdated": not_updated_details });
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Strip exclusive attributes before local update
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let local_fragment = if !exclusive_attrs.is_empty() {
        let mut filtered = fragment.clone(); // clone: need modified copy; original consumed by forwarding
        strip_exclusive_attrs_from_value(&mut filtered, &exclusive_attrs);
        filtered
    } else {
        fragment
    };

    let local_result = service.update_attrs(&id, local_fragment, &context).await;

    if local_result.is_ok() && !has_remote_errors(&forwarded) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let body = build_multi_status_body(&entity_id, local_result.is_ok(), local_result.err().as_ref(), &forwarded);
    Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
}

pub async fn partial_update_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id)): Path<(String, String)>,
    ValidatedQuery(params): ValidatedQuery<PartialUpdateAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    let context = context.merge(body_context);

    // Forward to matching context sources
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let fragment_body = serde_json::to_vec(&fragment).map_err(|e| {
        AndromedaError::internal("Failed to serialize fragment")
            .with_source(e)
            .with_instance(uri.clone())
    })?;
    let entity_id = id.to_string();
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_deref().map(|d| format!("datasetId={d}"));

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::PATCH,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::PartialUpdateAttribute,
            path_suffix: Some(&path_suffix),
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        service
            .partial_update_attr(&id, &attr_id, params.dataset_id.as_deref(), fragment, &context)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect || modes.has_exclusive {
        // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
        if modes.has_exclusive
            && let Err(e) = service.delete_attr(&id, &attr_id, None, true, &context).await
        {
            tracing::warn!(error = %e, entity_id = %id, attr_id = %attr_id, "Failed to delete exclusive attribute locally");
        }
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Inclusive: apply locally and return 204.
    // Local errors are non-fatal — the remote already succeeded.
    if let Err(e) = service
        .partial_update_attr(&id, &attr_id, params.dataset_id.as_deref(), fragment, &context)
        .await
    {
        tracing::warn!(error = %e, entity_id = %id, attr_id = %attr_id, "Failed to apply inclusive partial_update_attr locally");
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn delete_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id)): Path<(String, String)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    let context: ContextSpec = context.into_inner();
    let delete_all = params.delete_all.unwrap_or(false);

    // Forward to matching context sources
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_id = id.to_string();
    let path_suffix = format!("/attrs/{attr_id}");

    // Build query string for forwarding
    let mut qs_parts = Vec::new();
    if delete_all {
        qs_parts.push("deleteAll=true".to_string());
    }
    if let Some(ref ds) = params.dataset_id {
        qs_parts.push(format!("datasetId={ds}"));
    }
    let qs = if qs_parts.is_empty() { None } else { Some(qs_parts.join("&")) };

    let (forwarded, _) = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            method: reqwest::Method::DELETE,
            body: None,
            context_url,
            context: &context,
            operation: ForwardingOperation::DeleteEntityAttribute,
            path_suffix: Some(&path_suffix),
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    if forwarded.is_empty() {
        service
            .delete_attr(&id, &attr_id, params.dataset_id.as_deref(), delete_all, &context)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect || modes.has_exclusive {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Inclusive: delete locally and combine results
    let local_result = service.delete_attr(&id, &attr_id, params.dataset_id.as_deref(), delete_all, &context).await;

    if local_result.is_ok() && !has_remote_errors(&forwarded) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let body = build_multi_status_body(&entity_id, local_result.is_ok(), local_result.err().as_ref(), &forwarded);
    Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
}

pub async fn replace_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id)): Path<(String, String)>,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    let context = context.merge(body_context);

    service
        .replace_attr(&id, &attr_id, fragment, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// §6.4.3.3 – Delete entities matching a query.
pub async fn delete_by_query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    EntityQuery(query): EntityQuery,
) -> Result<Response, NgsiLdError> {
    let is_local = query.local.unwrap_or(false);
    let context: ContextSpec = context.into_inner();

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let entity_ids = state.entity_query.query_ids(&compiled).await.map_err(|e| e.with_instance(uri.clone()))?;

    if entity_ids.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    if !is_local {
        let body = serde_json::to_vec(&entity_ids).map_err(|e| {
            AndromedaError::internal("Failed to serialize entity IDs")
                .with_source(e)
                .with_instance(uri.clone())
        })?;
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let forwarded = state
            .forwarding
            .forward_batch_provision(ForwardBatchProvisionRequest {
                entity_type_compact: None,
                body: &body,
                context_url,
                context: &context,
                operation: ForwardingOperation::DeleteBatch,
                batch_path: "/entityOperations/delete",
                query_string: None,
                incoming_via: incoming_via.as_ref(),
            })
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        let modes = ForwardingModes::from_results(&forwarded);

        if modes.has_redirect {
            let Some(first) = forwarded.first() else {
                return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(uri)));
            };
            let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
            if let Some(ref resp_body) = first.body {
                return Ok((status, Json(resp_body.clone())).into_response());
            }
            return Ok(status.into_response());
        }

        if modes.has_exclusive {
            if let Err(e) = state.entities.batch_delete(&entity_ids).await {
                tracing::warn!(error = %e, "Failed to delete local entities during exclusive batch delete");
            }
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
    }

    let batch = state.entities.batch_delete(&entity_ids).await.map_err(|e| e.with_instance(uri.clone()))?;
    let result: BatchOperationResult = batch.into();

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}
