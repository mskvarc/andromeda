//! API handlers for the NGSI-LD Temporal API.

use crate::{
    api::{
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, params::ValidatedQuery, protocol::NgsiLdProtocolHeaders},
        response::ngsild::{NgsiLdCollectionResponse, NgsiLdResponse},
        tenant::TenantState,
    },
    domain::entity::Entity,
    error::{AndromedaError, api::ApiError, core::AndromedaErrorKind},
    jsonld::ContextSpec,
    query::temporal::TemporalQueryParameters,
    service::{
        context_source_forwarding::{ForwardingOperation, collect_warnings, merge_temporal_query_results, merge_temporal_retrieve_results},
        temporal::CreateOrUpdateResult,
    },
};
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use urn::Urn;

fn deserialize_bool_case_insensitive<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(s) => match s.to_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            other => Err(serde::de::Error::custom(format!("invalid boolean value: '{}'", other))),
        },
    }
}

fn parse_entity_id(id: &str, uri: &axum::http::Uri) -> Result<Urn, ApiError> {
    id.parse::<Urn>().map_err(|_| {
        ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("Invalid entity id: not a valid URI"),
            uri.clone(),
        )
    })
}

fn validate_attr_name(name: &str, uri: &axum::http::Uri) -> Result<(), ApiError> {
    if name.is_empty() || (!name.contains("://") && !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))) {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("Invalid attribute name"),
            uri.clone(),
        ));
    }
    Ok(())
}

fn validate_instance_id(id: &str, uri: &axum::http::Uri) -> Result<(), ApiError> {
    if id.is_empty() || url::Url::parse(id).is_err() {
        return Err(ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("Invalid instance id: not a valid URI"),
            uri.clone(),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteTemporalAttrParams {
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "deleteAll", default, deserialize_with = "deserialize_bool_case_insensitive")]
    pub delete_all: Option<bool>,
}

/// POST /ngsi-ld/v1/temporal/entities
pub async fn create_or_update(
    crate::api::tenant::TenantStateOrCreate(state): crate::api::tenant::TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<crate::api::handler::types::EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    let result = service.create_or_update(payload, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    match result {
        CreateOrUpdateResult::Created(id) => Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/temporal/entities/{}", id))]).into_response()),
        CreateOrUpdateResult::Updated => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// GET /ngsi-ld/v1/temporal/entities/{id}
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    let context: ContextSpec = context.into_inner();
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve temporal entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state
            .snapshots
            .validate_for_scoping(&snapshot_id)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        let entity_data = state
            .snapshots
            .get_snapshot_temporal_entity(&snapshot_id, id.as_ref())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?
            .ok_or_else(|| ApiError::from_error(AndromedaError::not_found(id.to_string()), uri.clone()))?;

        let options = params.options.as_deref().unwrap_or("");
        let key_values = options.contains("keyValues") || options.contains("simplified");

        if key_values {
            let entity: Entity = serde_json::from_value(entity_data).map_err(|e| {
                ApiError::from(AndromedaError::bad_request_data(format!(
                    "Failed to parse temporal entity for simplification: {}",
                    e
                )))
            })?;
            return Ok(NgsiLdResponse::ok(entity.simplify())
                .with_context(context)
                .with_format(accept.format())
                .with_snapshot_uri(snapshot_id)
                .into_response());
        }

        return Ok(NgsiLdResponse::ok(entity_data)
            .with_context(context)
            .with_format(accept.format())
            .with_snapshot_uri(snapshot_id)
            .into_response());
    }

    // EntityMap scoping (spec 5.7.3.4): if NGSILD-EntityMap header is present,
    // only entities in the map are accessible. Return 404 if not in the map.
    if let Some(ref map_id) = entity_map_header {
        let map = state.entity_maps.retrieve(map_id.as_str()).await.map_err(|e| e.with_instance(uri.clone()))?;
        if !map.entity_ids.contains(&id.to_string()) {
            return Err(ApiError::from_error(AndromedaError::not_found(id.to_string()), uri.clone()));
        }
    }

    let is_local = params.is_local();

    let local_result = service.get(id.as_ref(), &params, &context).await;

    let (entity_value, content_range, warnings) = if is_local {
        let result = local_result.map_err(|e| e.with_instance(uri.clone()))?;
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
            )
            .await
            .unwrap_or_default();
        let warnings = collect_warnings(&forwarded);

        let local_value = match local_result {
            Ok(result) => Some((result.value, result.content_range)),
            Err(ref e) if e.kind() == AndromedaErrorKind::NotFound => None,
            Err(e) => return Err(e.with_instance(uri.clone()).into()),
        };

        let (local_entity, content_range) = match local_value {
            Some((value, cr)) => (Some(value), cr),
            None => (None, None),
        };

        let merged = merge_temporal_retrieve_results(local_entity, forwarded);
        match merged {
            Some(value) => (value, content_range, warnings),
            None => {
                return Err(ApiError::from_error(AndromedaError::not_found(id.to_string()), uri.clone()));
            }
        }
    };

    let options = params.options.as_deref().unwrap_or("");
    let key_values = options.contains("keyValues") || options.contains("simplified");

    let status = if content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    if key_values {
        let entity: Entity = serde_json::from_value(entity_value).map_err(|e| {
            ApiError::from(AndromedaError::bad_request_data(format!(
                "Failed to parse temporal entity for simplification: {}",
                e
            )))
        })?;

        let mut builder = NgsiLdResponse::new(status, entity.simplify())
            .with_context(context)
            .with_format(accept.format())
            .with_warnings(warnings);
        if let Some(map_header) = entity_map_header {
            builder = builder.with_entity_map_uri(map_header.into_inner());
        }
        let mut response = builder.into_response();
        if let Some(cr) = content_range
            && let Ok(val) = cr.parse()
        {
            response.headers_mut().insert("Content-Range", val);
        }
        Ok(response)
    } else {
        let mut builder = NgsiLdResponse::new(status, entity_value)
            .with_context(context)
            .with_format(accept.format())
            .with_warnings(warnings);
        if let Some(map_header) = entity_map_header {
            builder = builder.with_entity_map_uri(map_header.into_inner());
        }
        let mut response = builder.into_response();
        if let Some(cr) = content_range
            && let Ok(val) = cr.parse()
        {
            response.headers_mut().insert("Content-Range", val);
        }
        Ok(response)
    }
}

/// GET /ngsi-ld/v1/temporal/entities
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;
    let options = params.options.as_deref().unwrap_or("");
    let key_values = options.contains("keyValues") || options.contains("simplified");

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, query temporal entities from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state
            .snapshots
            .validate_for_scoping(&snapshot_id)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let limit = params.limit.unwrap_or(100) as i64;
        let offset = params.offset.unwrap_or(0) as i64;

        let (entities, total) = state
            .snapshots
            .query_snapshot_temporal_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        if key_values {
            let mut simplified = Vec::with_capacity(entities.len());
            for e_val in entities {
                let entity: Entity = serde_json::from_value(e_val).map_err(|e| {
                    ApiError::from(AndromedaError::bad_request_data(format!(
                        "Failed to parse temporal entity for simplification: {}",
                        e
                    )))
                })?;
                simplified.push(entity.simplify());
            }
            let response = NgsiLdCollectionResponse::ok(simplified)
                .with_context(context)
                .with_format(accept.format())
                .with_snapshot_uri(snapshot_id)
                .with_total_count(total as usize);
            return Ok(response.into_response());
        }

        let response = NgsiLdCollectionResponse::ok(entities)
            .with_context(context)
            .with_format(accept.format())
            .with_snapshot_uri(snapshot_id)
            .with_total_count(total as usize);
        return Ok(response.into_response());
    }

    // EntityMap scoping (spec 5.5.14): if NGSILD-EntityMap header is present,
    // scope the query to only the entity IDs in the map.
    if let Some(ref map_id) = entity_map_header {
        let map = state.entity_maps.retrieve(map_id.as_str()).await.map_err(|e| e.with_instance(uri.clone()))?;
        let ids_csv = map.entity_ids.join(",");
        if let Some(ref existing_ids) = params.id {
            // Intersect: keep only IDs that are in both the query and the map
            let query_ids: std::collections::HashSet<String> = existing_ids.split(',').map(|s| s.trim().to_string()).collect();
            let map_ids: std::collections::HashSet<String> = map.entity_ids.into_iter().collect();
            let intersection: Vec<String> = query_ids.intersection(&map_ids).cloned().collect();
            params.id = Some(intersection.join(","));
        } else {
            params.id = Some(ids_csv);
        }
    }

    let is_local = params.is_local();

    let result = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    for warning in &result.warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal query warning");
    }
    let crate::service::temporal::TemporalQueryResult {
        entities: local_entities,
        total_count,
        any_truncated,
        content_range,
        warnings: _,
    } = result;

    // Distributed forwarding: query matching context sources unless local=true
    let (entities, forwarding_warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();
        let query_string = uri.query().unwrap_or("");

        let forwarded = state
            .forwarding
            .forward_temporal_query(&crate::service::context_source_forwarding::ForwardTemporalQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids_ref,
                query_string,
                context_url,
                context: &context,
                operation: ForwardingOperation::QueryTemporalEntity,
                incoming_via: incoming_via.as_ref(),
            })
            .await
            .unwrap_or_default();
        let warnings = collect_warnings(&forwarded);

        if forwarded.is_empty() {
            (local_entities, warnings)
        } else {
            let merged = merge_temporal_query_results(local_entities, forwarded);
            (merged, warnings)
        }
    };

    // EntityMap creation (spec 5.5.14, 5.7.4.4): if entityMap=true was requested,
    // create a new EntityMap from all matching IDs (not just the current page).
    // splitEntities is accepted for API compatibility but not yet used for temporal
    // post-merge filtering (the entity query handler implements this in full).
    let _split_entities = params.split_entities;
    let entity_map_requested = params.entity_map.unwrap_or(false);
    let entity_map_uri = if entity_map_requested {
        let all_ids = service.query_ids(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
        let map = state
            .entity_maps
            .create(all_ids, params.entity_map_lifetime.as_deref())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;
        Some(map.id)
    } else if entity_map_header.is_some() {
        entity_map_header.map(|h| h.into_inner())
    } else {
        None
    };

    let status = if entity_map_requested { StatusCode::CREATED } else { StatusCode::OK };
    let returned = entities.len();

    if key_values {
        let mut simplified = Vec::with_capacity(entities.len());
        for e_val in entities {
            let entity: Entity = serde_json::from_value(e_val).map_err(|e| {
                ApiError::from(AndromedaError::bad_request_data(format!(
                    "Failed to parse temporal entity for simplification: {}",
                    e
                )))
            })?;
            simplified.push(entity.simplify());
        }

        let mut response = NgsiLdCollectionResponse::ok(simplified).with_status(status);
        response = response.with_context(context).with_format(accept.format()).with_warnings(forwarding_warnings);
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
    } else {
        let mut response = NgsiLdCollectionResponse::ok(entities).with_status(status);
        response = response.with_context(context).with_format(accept.format()).with_warnings(forwarding_warnings);
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
}

/// DELETE /ngsi-ld/v1/temporal/entities/{id}
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(id): Path<String>) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    service.delete(id.as_ref()).await.map_err(|e| e.with_instance(uri.clone()))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// POST /ngsi-ld/v1/temporal/entities/{id}/attrs
pub async fn add_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    service
        .add_attrs(id.as_ref(), payload, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/temporal/entities/{id}/attrs/{attr_id}
pub async fn delete_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id)): Path<(String, String)>,
    ValidatedQuery(params): ValidatedQuery<DeleteTemporalAttrParams>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    let context = context.into_inner();
    service
        .delete_attr(
            id.as_ref(),
            &attr_id,
            params.dataset_id.as_deref(),
            params.delete_all.unwrap_or(false),
            &context,
        )
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// PATCH /ngsi-ld/v1/temporal/entities/{id}/attrs/{attr_id}/{instance_id}
pub async fn modify_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id, instance_id)): Path<(String, String, String)>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ApiError> {
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
        .map_err(|e| e.with_instance(uri.clone()))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/temporal/entities/{id}/attrs/{attr_id}/{instance_id}
pub async fn delete_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id, instance_id)): Path<(String, String, String)>,
    ValidatedQuery(_params): ValidatedQuery<crate::api::handler::types::EmptyParams>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    validate_instance_id(&instance_id, &uri)?;
    let context: ContextSpec = context.into_inner();
    service
        .delete_attr_instance(id.as_ref(), &attr_id, &instance_id, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
