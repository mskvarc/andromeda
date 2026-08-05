//! API handlers for the NGSI-LD Temporal API.

use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    core::error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{
            accept::AcceptFormatNoGeo,
            context::ContextUrl,
            jsonld::JsonLd,
            prefer::PreferHeader,
            protocol::NgsiLdProtocolHeaders,
            query::ValidatedQuery,
        },
        model::deferred_split_filters::DeferredSplitFilters,
        parameter::{DeleteAttrParams, EmptyParams, protocol::apply_temporal_extension_headers, query::temporal::TemporalQueryParameters},
        response::{collection::NgsiLdCollectionResponse, single::NgsiLdResponse},
        validation::{parse_entity_id, validate_attr_name, validate_instance_id},
    },
    jsonld::{ContextSpec, term_expansion::ContextualExpander},
    renderer::{
        Renderer,
        response::{ResponsePayload, ResponseRenderer},
        temporal::simplify::{TemporalSimplifyPayload, TemporalSimplifyRenderer},
    },
    repository::temporal::params::DeleteScope,
    service::{
        csource_forwarding::{CsrModeFilter, ForwardTemporalQueryRequest, ForwardingOperation, collect_temporal_warnings, merge_temporal_retrieve_results},
        entity_map::{parse_entity_map_urn, parse_iso8601_duration, parse_uri_csv},
        subscription_cache::compile_geo_q,
        subscription_matching::{matches_compiled_geo_query, matches_scope_query},
        temporal::{CreateOrUpdateResult, TemporalQueryResult, TemporalService},
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate, expansion::expand_q_expression},
    scope::query::ScopeQuery,
};
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::str::FromStr;

/// Handles `POST /ngsi-ld/v1/temporal/entities`.
///
/// Creates a new temporal entity or updates an existing one from the request payload.
pub async fn create_or_update(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    let result = service.create_or_update(payload, &context).await.with_instance(&uri)?;

    match result {
        CreateOrUpdateResult::Created(id) => Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/temporal/entities/{}", id))]).into_response()),
        CreateOrUpdateResult::Updated => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// Handles `GET /ngsi-ld/v1/temporal/entities/{id}`.
///
/// Retrieves a single temporal entity by its identifier, with support for snapshot
/// scoping, entity-map scoping, and distributed context-source forwarding.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    Path(id): Path<String>,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    let context: ContextSpec = context.into_inner();
    apply_temporal_extension_headers(&mut params, &protocol, &uri)?;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve temporal entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_data = state
            .snapshots
            .get_snapshot_temporal_entity(&snapshot_id, id.as_ref())
            .await
            .with_instance(&uri)?
            .ok_or_else(|| NgsiLdError::from(AndromedaError::temporal_not_found(id.to_string()).with_instance(&uri)))?;

        let key_values = params.is_simplified();

        let data = if key_values {
            TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(entity_data))
                .render()
                .with_instance(&uri)?
                .into_single()
                .with_instance(&uri)?
        } else {
            entity_data
        };
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(data))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).with_snapshot_uri(snapshot_id).into_response());
    }

    // EntityMap scoping (spec 5.7.3.4): if NGSILD-EntityMap header is present,
    // only entities in the map are accessible. Return 404 if not in the map.
    if let Some(ref map_id) = entity_map_header {
        let map_urn = parse_entity_map_urn(map_id.as_str()).with_instance(&uri)?;
        state.entity_maps.validate_entity_in_map(&map_urn, &id).await.with_instance(&uri)?;
    }

    let is_local = params.is_local();

    let local_result = service.get(id.as_ref(), &params, &context).await;

    let (entity_value, content_range, warnings) = if is_local {
        let result = local_result.with_instance(&uri)?;
        (result.value, result.content_range, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_temporal_retrieve(
                id.as_ref(),
                query_string,
                context_url,
                ForwardingOperation::RetrieveTemporalEntity,
                incoming_via.as_ref(),
                CsrModeFilter::All,
            )
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        let local_value = match local_result {
            Ok(result) => Some((result.value, result.content_range)),
            Err(ref e) if e.is_not_found() => None,
            Err(e) => return Err(e.with_instance(&uri).into()),
        };

        let (local_entity, content_range) = match local_value {
            Some((value, cr)) => (Some(value), cr),
            None => (None, None),
        };

        let merged = merge_temporal_retrieve_results(local_entity, forwarded);
        match merged {
            Some(value) => (value, content_range, warnings),
            None => {
                return Err(NgsiLdError::from(AndromedaError::temporal_not_found(id.to_string()).with_instance(&uri)));
            }
        }
    };

    let key_values = params.is_simplified();

    let status = if content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    let data: Value = if key_values {
        TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(entity_value))
            .render()
            .with_instance(&uri)?
            .into_single()
            .with_instance(&uri)?
    } else {
        entity_value
    };

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(data))
        .render()?;
    let mut builder = NgsiLdResponse::new(status, rendered).with_warnings(warnings);
    if let Some(map_header) = entity_map_header {
        builder = builder.with_entity_map_uri(map_header.into_inner().to_string());
    }
    if let Some(cr) = content_range {
        builder = builder.with_content_range(cr);
    }
    Ok(builder.into_response())
}

/// Handles `GET /ngsi-ld/v1/temporal/entities`.
///
/// Queries temporal entities matching the supplied parameters, with support for
/// snapshot scoping, entity-map scoping, and distributed context-source forwarding.
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
    apply_temporal_extension_headers(&mut params, &protocol, &uri)?;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;
    let key_values = params.is_simplified();

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, query temporal entities from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
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
        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
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
        let map_urn = parse_entity_map_urn(map_id.as_str()).with_instance(&uri)?;
        let scoped_input = params.id.as_deref().map(parse_uri_csv).transpose().with_instance(&uri)?;
        let scoped = state.entity_maps.scope_query_ids(&map_urn, scoped_input.as_deref()).await.with_instance(&uri)?;
        params.id = Some(scoped.iter().map(ToString::to_string).collect::<Vec<_>>().join(","));

        // EntityMap-scoped temporal queries may omit temporal bounds; default to
        // an "all history up to far future" window so scoped follow-up queries succeed.
        if params.timerel.is_none() && params.time_at.is_none() {
            params.timerel = Some("before".to_string());
            params.time_at = Some("9999-12-31T23:59:59Z".to_string());
        }
    }

    let is_local = params.is_local();
    let entity_map_requested = params.entity_map.unwrap_or(false);
    let split_entities = params.split_entities.unwrap_or(false) && !is_local;

    // splitEntities (spec 5.7.4.4): defer q/geoQ/scopeQ filtering to post-merge
    let deferred = if split_entities {
        DeferredSplitFilters {
            q: params.q.take(),
            georel: params.georel.take(),
            geometry: params.geometry.take(),
            coordinates: params.coordinates.take(),
            geoproperty: params.geoproperty.take(),
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
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
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
        let q_expr = if let Some(ref q_str) = deferred.q {
            expand_q_expression(Some(q_str), &ContextualExpander::new(service.engine(), &context))
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        let geo_val = if let (Some(georel), Some(geometry), Some(coordinates)) = (&deferred.georel, &deferred.geometry, &deferred.coordinates) {
            let mut value = serde_json::json!({
                "georel": georel,
                "geometry": geometry,
                "coordinates": coordinates,
            });
            if let Some(ref geoprop) = deferred.geoproperty {
                value["geoproperty"] = Value::String(geoprop.clone());
            }
            Some(value)
        } else {
            None
        };
        let compiled_scope = deferred
            .scope_q
            .as_deref()
            .map(ScopeQuery::parse)
            .transpose()
            .map_err(AndromedaError::from)
            .with_instance(&uri)?;
        let compiled_geo = geo_val.as_ref().and_then(|v| compile_geo_q(v, "split-entities"));

        let mut filtered = Vec::with_capacity(entities.len());
        for entity in entities {
            let expanded = match service.engine().expand(entity.clone(), &context).await {
                // clone: `expand` consumes `Value`, and we must keep original compact value for response
                Ok(value) => value,
                Err(_) => continue,
            };

            let q_ok = q_expr.as_ref().is_none_or(|expr| evaluate(expr, &expanded));
            let geo_ok = compiled_geo.as_ref().is_none_or(|geo| matches_compiled_geo_query(geo, &expanded));
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
        let lifetime = params
            .entity_map_lifetime
            .as_deref()
            .map(|raw| parse_iso8601_duration(Some(raw)))
            .transpose()
            .with_instance(&uri)?;
        let map = state.entity_maps.create(typed_ids, lifetime.as_ref()).await.with_instance(&uri)?;
        Some(map.id.to_string())
    } else if entity_map_header.is_some() {
        entity_map_header.map(|h| h.into_inner().to_string())
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

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default())
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

/// Handles `DELETE /ngsi-ld/v1/temporal/entities/{id}`.
///
/// Deletes a temporal entity and all of its temporal attribute instances.
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(id): Path<String>) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    service.delete(id.as_ref()).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Handles `POST /ngsi-ld/v1/temporal/entities/{id}/attrs`.
///
/// Appends one or more temporal attributes to an existing temporal entity.
pub async fn add_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    service.add_attrs(id.as_ref(), payload, &context).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Handles `DELETE /ngsi-ld/v1/temporal/entities/{id}/attrs/{attr_id}`.
///
/// Deletes a temporal attribute from a temporal entity, either a single default
/// instance or all instances when `deleteAll` is specified.
pub async fn delete_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id)): Path<(String, String)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    let context = context.into_inner();
    let scope = if params.delete_all.unwrap_or(false) {
        DeleteScope::All
    } else {
        DeleteScope::Single
    };
    service
        .delete_attr(id.as_ref(), &attr_id, params.dataset_id.as_deref(), scope, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Handles `PATCH /ngsi-ld/v1/temporal/entities/{id}/attrs/{attr_id}/{instance_id}`.
///
/// Updates a specific temporal attribute instance identified by its instance ID.
pub async fn update_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id, instance_id)): Path<(String, String, String)>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    if attr_id.is_empty() {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    validate_attr_name(&attr_id, &uri)?;
    validate_instance_id(&instance_id, &uri)?;
    let context = context.merge(body_context);
    service
        .modify_attr_instance(id.as_ref(), &attr_id, &instance_id, payload, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Handles `DELETE /ngsi-ld/v1/temporal/entities/{id}/attrs/{attr_id}/{instance_id}`.
///
/// Deletes a specific temporal attribute instance identified by its instance ID.
pub async fn delete_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id, instance_id)): Path<(String, String, String)>,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    validate_instance_id(&instance_id, &uri)?;
    let context: ContextSpec = context.into_inner();
    service
        .delete_attr_instance(id.as_ref(), &attr_id, &instance_id, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
