use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    core::error::{AndromedaError, ResultInstanceExt},
    domain::{
        entity::{Entity, EntityPatch, EntityTypeValue},
        uri::Uri,
    },
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
            entity::{AppendParams, GetEntityParams, PartialUpdateAttrParams, ReplaceAttrParams},
            protocol::{apply_entity_get_extension_headers, apply_entity_query_extension_headers},
        },
        response::{collection::NgsiLdCollectionResponse, single::NgsiLdResponse},
        validation::{parse_entity_id, validate_attr_name},
    },
    jsonld::ContextSpec,
    query::{compile::expand_q_expression, compiled::PurgeMode, q::eval::evaluate},
    renderer::{
        Renderer,
        SysAttrsInclusion,
        entity::EntityRenderer,
        response::{ResponsePayload, ResponseRenderer},
        snapshot::SnapshotRenderer,
    },
    service::{
        context_source_forwarding::{
            CsrModeFilter,
            ForwardBatchProvisionRequest,
            ForwardProvisionRequest,
            ForwardQueryRequest,
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
        entity_map::{parse_entity_map_urn, parse_iso8601_duration, parse_uri_csv},
        subscription_cache::{compile_geo_q, compile_scope_q},
        subscription_matching::{matches_compiled_geo_query, matches_compiled_scope_pattern},
    },
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::{collections::HashSet, str::FromStr};

pub async fn patch_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    Err(NgsiLdError::from(
        AndromedaError::entity_validation("Entity id is required").with_instance(&uri),
    ))
}

pub async fn put_no_id(OriginalUri(uri): OriginalUri) -> Result<Response, NgsiLdError> {
    Err(NgsiLdError::from(
        AndromedaError::entity_validation("Entity id is required").with_instance(&uri),
    ))
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
        let id = service.create(&entity, &context).await.with_instance(&uri)?;
        return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response());
    }

    // Check for matching CSRs for distributed forwarding
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_body = serde_json::to_vec(&entity).map_err(|e| AndromedaError::internal(format!("Failed to serialize entity: {e}")).with_instance(&uri))?;
    let entity_id = entity.id.to_string();
    let entity_type_str: &str = match &entity.type_ {
        EntityTypeValue::Single(s) => s.as_str(),
        EntityTypeValue::Multiple(v) => v.first().map_or("", String::as_str),
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        let id = service.create(&entity, &context).await.with_instance(&uri)?;
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

    let id = service.create(&entity, &context).await.with_instance(&uri)?;
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
                AndromedaError::malformed_request(format!("Invalid options value: '{}'", opt)).with_instance(&uri),
            ));
        }
    }

    // Validate format parameter
    if let Some(ref fmt) = params.format
        && !matches!(fmt.as_str(), "normalized" | "keyValues" | "simplified" | "concise")
    {
        return Err(NgsiLdError::from(
            AndromedaError::malformed_request(format!("Invalid format value: '{}'", fmt)).with_instance(&uri),
        ));
    }

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_data = state
            .snapshots
            .get_snapshot_entity(&snapshot_id, id.as_ref())
            .await
            .with_instance(&uri)?
            .ok_or_else(|| NgsiLdError::from(AndromedaError::entity_not_found(id.to_string()).with_instance(&uri)))?;

        let mode = params.resolve_render_mode();
        let attrs: Option<Vec<&str>> = params.attrs.as_deref().map(|s| s.split(',').map(str::trim).collect());
        let result = SnapshotRenderer::new(entity_data)
            .with_attrs_filter(attrs.as_deref())
            .with_mode(mode)
            .render()?;

        let rendered = ResponseRenderer::new(accept.format(), context, prefer)
            .with_geometry_property(params.geometry_property.clone())
            .with_payload(ResponsePayload::Single(result))
            .render()?;
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
        let entity = local_result.with_instance(&uri)?;
        (entity, Vec::new())
    } else {
        // Forward to context sources
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        // Phase 1: forward to primary (non-auxiliary) CSRs
        let forwarded = state
            .forwarding
            .forward_retrieve(
                id.as_ref(),
                query_string,
                context_url,
                ForwardingOperation::RetrieveEntity,
                incoming_via.as_ref(),
                CsrModeFilter::ExcludeAuxiliary,
            )
            .await
            .unwrap_or_default(); // Retrieve forwarding: best-effort -- missing CSR data is not fatal
        let mut warnings = collect_warnings(&forwarded);

        let local_entity = local_result.ok();

        let merged = merge_retrieve_results(local_entity, forwarded);

        // Phase 2: forward to auxiliary CSRs only if an entity was found
        // Per spec 4.3.6.2: auxiliary context sources only supplement existing data
        let merged = if merged.is_some() {
            let aux_forwarded = state
                .forwarding
                .forward_retrieve(
                    id.as_ref(),
                    query_string,
                    context_url,
                    ForwardingOperation::RetrieveEntity,
                    incoming_via.as_ref(),
                    CsrModeFilter::AuxiliaryOnly,
                )
                .await
                .unwrap_or_default();
            if !aux_forwarded.is_empty() {
                warnings.extend(collect_warnings(&aux_forwarded));
                merge_retrieve_results(merged, aux_forwarded)
            } else {
                merged
            }
        } else {
            merged
        };
        let entity = match merged {
            Some(entity) => entity,
            None => {
                return Err(NgsiLdError::from(AndromedaError::entity_not_found(id.to_string()).with_instance(&uri)));
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
            .with_instance(&uri)?;

        if join_mode == Some("flat") {
            // Flat: always return array of [root, ...linked] per NGSI-LD spec
            let mut all_entities: Vec<Value> = Vec::new();
            all_entities.push(
                EntityRenderer::new(&entity)
                    .with_optional_language_filter(language_filter.as_ref())
                    .with_mode(mode)
                    .render()
                    .with_instance(&uri)?,
            );
            for linked_entity in linked.values() {
                all_entities.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_payload(ResponsePayload::Collection(all_entities))
                .render()?;
            return Ok(NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings).into_response());
        } else {
            // Inline: renderer handles join + mode transformation
            let entity_value = EntityRenderer::new(&entity)
                .with_optional_language_filter(language_filter.as_ref())
                .with_inline_linked_entities(&linked)
                .with_mode(mode)
                .render()
                .with_instance(&uri)?;
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_geometry_property(params.geometry_property.clone())
                .with_payload(ResponsePayload::Single(entity_value))
                .render()?;
            return Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response());
        }
    }

    let entity_value = EntityRenderer::new(&entity)
        .with_optional_language_filter(language_filter.as_ref())
        .with_mode(mode)
        .render()
        .with_instance(&uri)?;
    let entity_value = attach_proof_verification(entity_value, &entity, params.verify.unwrap_or(false), &state.provenance).await;
    let rendered = ResponseRenderer::new(accept.format(), context, prefer)
        .with_geometry_property(params.geometry_property.clone())
        .with_payload(ResponsePayload::Single(entity_value))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}

/// Attach an `ngsildproofVerification` sidecar to a rendered entity
/// JSON when the client opted in via `verify=true`.
async fn attach_proof_verification(mut rendered: Value, entity: &Entity, verify: bool, service: &crate::service::provenance::ProvenanceService) -> Value {
    if !verify {
        return rendered;
    }
    let results = service.verify_entity(entity).await;
    if let Value::Object(ref mut obj) = rendered
        && let Ok(value) = serde_json::to_value(&results)
    {
        obj.insert("ngsildproofVerification".to_string(), value);
    }
    rendered
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
        service.delete(&id).await.with_instance(&uri)?;
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
        .with_instance(&uri)?;

    // Per spec 4.3.6.3: if an exclusive CSR exists but doesn't support this operation,
    // delete locally but return 207 to indicate exclusive data could not be cleaned up.
    if forwarded.is_empty() && !unforwardable_exclusive.is_empty() {
        let local_result = service.delete(&id).await;
        let body = build_multi_status_body(&entity_id, local_result.is_ok(), local_result.err().as_ref(), &[]);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    if forwarded.is_empty() {
        service.delete(&id).await.with_instance(&uri)?;
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

    service.delete(&id).await.with_instance(&uri)?;
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
    Box::pin(async move {
    let service = &state.entity_query;
    let context: ContextSpec = context.into_inner();
    apply_entity_query_extension_headers(&mut query, &protocol, &uri)?;
    query.validate().with_instance(&uri).map_err(NgsiLdError::from)?;
    let language_filter = protocol.language_filter;
    let prefer = protocol.prefer;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, query entities from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_type = query.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = query.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let limit = query.limit.unwrap_or(100) as i64;
        let offset = query.offset.unwrap_or(0) as i64;

        let (entities, total) = state
            .snapshots
            .query_snapshot_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_instance(&uri)?;

        let opts = query.parse_options();
        let attrs: Option<Vec<&str>> = query.attrs.as_deref().map(|s| s.split(',').map(str::trim).collect());
        let filtered: Vec<Value> = entities
            .into_iter()
            .map(|v| {
                SnapshotRenderer::new(v)
                    .with_attrs_filter(attrs.as_deref())
                    .with_mode(opts.render_mode)
                    .render()
            })
            .collect::<Result<Vec<_>, _>>()?;

        let rendered = ResponseRenderer::new(accept.format(), context, prefer)
            .with_geometry_property(query.geometry_property.clone())
            .with_payload(ResponsePayload::Collection(filtered))
            .render()?;
        let response = NgsiLdCollectionResponse::ok(rendered)
            .with_snapshot_uri(snapshot_id)
            .with_total_count(total as usize);

        return Ok(response.into_response());
    }

    // §5.5.9.3: If NGSILD-EntityMap header present, scope query to that map's entity IDs
    if let Some(ref map_id) = entity_map_header {
        let map_urn = parse_entity_map_urn(map_id.as_str()).with_instance(&uri)?;
        let scoped_input = query.id.as_deref().map(parse_uri_csv).transpose().with_instance(&uri)?;
        let scoped = state.entity_maps.scope_query_ids(&map_urn, scoped_input.as_deref()).await.with_instance(&uri)?;
        query.id = Some(scoped.iter().map(ToString::to_string).collect::<Vec<_>>().join(","));
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

    let (local_entities, total_count) = service.query(&query, &context).await.with_instance(&uri)?;

    // Distributed forwarding: query matching context sources unless local=true
    let mut remote_ids: Vec<String> = Vec::new();
    let (mut entities, warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = query.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = query.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();

        // Parse csf for CSR filtering during forwarding
        let csf_expr = expand_q_expression(query.csf.as_deref(), state.entity_query.engine(), &context)
            .await
            .with_instance(&uri)?;

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
        remote_ids = forwarded.iter().flat_map(|f| f.entities.iter()).map(|e| e.id.to_string()).collect();

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
            let aux_remote_ids: Vec<String> = aux_forwarded.iter().flat_map(|f| f.entities.iter()).map(|e| e.id.to_string()).collect();
            remote_ids.extend(aux_remote_ids);
            let merged = EntityQueryService::merge_with_forwarded(merged, aux_forwarded, query.attrs.as_deref());
            (merged, warnings)
        } else {
            (merged, warnings)
        }
    };

    // splitEntities post-merge filtering: apply deferred q/geoQ/scopeQ (spec 5.7.2.4)
    if split_entities {
        let q_expr = if let Some(ref q_str) = deferred_q {
            expand_q_expression(Some(q_str), service.engine(), &context).await.ok().flatten()
        } else {
            None
        };
        let geo_val = if let (Some(georel), Some(geometry), Some(coordinates)) = (&deferred_georel, &deferred_geometry, &deferred_coordinates) {
            let mut value = serde_json::json!({
                "georel": georel,
                "geometry": geometry,
                "coordinates": coordinates,
            });
            if let Some(ref geoprop) = deferred_geoproperty {
                value["geoproperty"] = Value::String(geoprop.clone());
            }
            Some(value)
        } else {
            None
        };
        let compiled_scope = deferred_scope_q.as_deref().map(compile_scope_q);
        let compiled_geo = geo_val.as_ref().and_then(|v| compile_geo_q(v, "split-entities"));

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
            let geo_ok = compiled_geo.as_ref().is_none_or(|geo| matches_compiled_geo_query(geo, &expanded));
            let scope_ok = compiled_scope.as_ref().is_none_or(|scope| matches_compiled_scope_pattern(scope, &expanded));

            if q_ok && geo_ok && scope_ok {
                filtered.push(entity);
            }
        }
        entities = filtered;
    }

    // §5.7.2: If entityMap=true, create an EntityMap and set header
    let entity_map_uri = if entity_map_requested {
        let compiled = service.compile_query_public(&query, &context).await.with_instance(&uri)?;
        let mut all_ids = service.query_ids(&compiled).await.with_instance(&uri)?;
        // Include forwarded entity IDs in the map (spec 5.7.2.4, 5.5.14)
        all_ids.extend(remote_ids);
        all_ids.sort_unstable();
        all_ids.dedup();
        let typed_ids: Vec<Uri> = all_ids
            .iter()
            .map(|id| Uri::from_str(id).map_err(|_| AndromedaError::entity_validation(format!("Invalid entity id URI '{}'", id))))
            .collect::<Result<Vec<_>, _>>()
            .with_instance(&uri)?;
        let lifetime = query
            .entity_map_lifetime
            .as_deref()
            .map(|raw| parse_iso8601_duration(Some(raw)))
            .transpose()
            .with_instance(&uri)?;
        let map = state.entity_maps.create(typed_ids, lifetime.as_ref()).await.with_instance(&uri)?;
        Some(map.id.to_string())
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
            .with_instance(&uri)?;

        if join_mode == Some("flat") {
            let mut all_values: Vec<Value> = Vec::new();
            for entity in &entities {
                all_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            for linked_entity in linked.values() {
                all_values.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_geometry_property(query.geometry_property.clone())
                .with_payload(ResponsePayload::Collection(all_values))
                .render()?;
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
                        .with_instance(&uri)?,
                );
            }
            let rendered = ResponseRenderer::new(accept.format(), context, prefer)
                .with_geometry_property(query.geometry_property.clone())
                .with_payload(ResponsePayload::Collection(entity_values))
                .render()?;
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
        .with_instance(&uri)?;
    let rendered_output = ResponseRenderer::new(accept.format(), context, prefer)
        .with_geometry_property(query.geometry_property.clone())
        .with_payload(ResponsePayload::Collection(rendered))
        .render()?;
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
        service.merge(&id, &patch, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let patch_body = serde_json::to_vec(&patch).map_err(|e| AndromedaError::internal(format!("Failed to serialize patch: {e}")).with_instance(&uri))?;
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        service.merge(&id, &patch, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let mut patch = patch;
    if !exclusive_attrs.is_empty() && !patch.strip_exclusive_attrs(&exclusive_attrs) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    service.merge(&id, &patch, &context).await.with_instance(&uri)?;
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
        service.replace_entity(&id, &patch, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let patch_body = serde_json::to_vec(&patch).map_err(|e| AndromedaError::internal(format!("Failed to serialize patch: {e}")).with_instance(&uri))?;
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        service.replace_entity(&id, &patch, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let mut patch = patch;
    if !exclusive_attrs.is_empty() {
        patch.strip_exclusive_attrs(&exclusive_attrs);
    }
    service.replace_entity(&id, &patch, &context).await.with_instance(&uri)?;
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
    let fragment_body =
        serde_json::to_vec(&fragment).map_err(|e| AndromedaError::internal(format!("Failed to serialize fragment: {}", e)).with_instance(&uri))?;
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — append locally
        let (all_attrs, not_appended) = service.append_attrs(&id, fragment, no_overwrite, &context).await.with_instance(&uri)?;

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
    let mut local_fragment = fragment;
    if !exclusive_attrs.is_empty() {
        strip_exclusive_attrs_from_value(&mut local_fragment, &exclusive_attrs);
    }

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
    let fragment_body =
        serde_json::to_vec(&fragment).map_err(|e| AndromedaError::internal(format!("Failed to serialize fragment: {}", e)).with_instance(&uri))?;
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        let not_updated = service.update_attrs(&id, fragment, &context).await.with_instance(&uri)?;
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
    let mut local_fragment = fragment;
    if !exclusive_attrs.is_empty() {
        strip_exclusive_attrs_from_value(&mut local_fragment, &exclusive_attrs);
    }

    // Exclusive-only updates may strip all local attributes; in that case
    // local processing is complete and only remote status matters.
    if local_fragment.as_object().is_some_and(|obj| obj.is_empty()) {
        if !has_remote_errors(&forwarded) {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let body = build_multi_status_body(&entity_id, true, None, &forwarded);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

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
    let fragment_body =
        serde_json::to_vec(&fragment).map_err(|e| AndromedaError::internal(format!("Failed to serialize fragment: {}", e)).with_instance(&uri))?;
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        service
            .partial_update_attr(&id, &attr_id, params.dataset_id.as_deref(), fragment, &context)
            .await
            .with_instance(&uri)?;
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
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        service
            .delete_attr(&id, &attr_id, params.dataset_id.as_deref(), delete_all, &context)
            .await
            .with_instance(&uri)?;
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
    ValidatedQuery(params): ValidatedQuery<ReplaceAttrParams>,
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
        .replace_attr(&id, &attr_id, params.dataset_id.as_deref(), fragment, &context)
        .await
        .with_instance(&uri)?;

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

    // Determine purge mode from drop/keep parameters (S5.6.21)
    let purge_mode = if let Some(ref drop_attrs) = query.drop {
        PurgeMode::Drop(drop_attrs.split(',').map(|s| s.trim().to_string()).collect())
    } else if let Some(ref keep_attrs) = query.keep {
        PurgeMode::Keep(keep_attrs.split(',').map(|s| s.trim().to_string()).collect())
    } else {
        PurgeMode::DeleteEntities
    };

    let compiled = state.entity_query.compile_query_uncapped(&query, &context).await.with_instance(&uri)?;
    let entity_ids = state.entity_query.query_ids(&compiled).await.with_instance(&uri)?;

    if entity_ids.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // When drop/keep is specified, modify attributes instead of deleting entities
    match purge_mode {
        PurgeMode::Drop(_) | PurgeMode::Keep(_) => {
            state.entities.purge_attrs(&entity_ids, &purge_mode, &context).await.with_instance(&uri)?;
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        PurgeMode::DeleteEntities => {}
    }

    if !is_local {
        let body =
            serde_json::to_vec(&entity_ids).map_err(|e| AndromedaError::internal(format!("Failed to serialize entity IDs: {}", e)).with_instance(&uri))?;
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
            .with_instance(&uri)?;

        let modes = ForwardingModes::from_results(&forwarded);

        if modes.has_redirect {
            let Some(first) = forwarded.first() else {
                return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
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

    let batch = state.entities.batch_delete(&entity_ids).await.with_instance(&uri)?;
    let result: BatchOperationResult = batch.into();

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}
