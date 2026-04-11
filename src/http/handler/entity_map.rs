//! Handlers for the `/entityMaps` endpoints.

use crate::{
    api::tenant::TenantState,
    domain::uri::Uri,
    error::AndromedaError,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, entity_query::EntityQuery, prefer::PreferHeader},
        model::entity_map_query::{QueryRequestBody, UpdateEntityMapBody, query_body_to_params, validate_has_filter},
        response::single::NgsiLdResponse,
        validation::parse_entity_map_id,
    },
    jsonld::ContextSpec,
    renderer::response::ResponseRenderer,
    service::entity_map::parse_iso8601_duration,
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::DateTime;
use std::str::FromStr;

/// GET /ngsi-ld/v1/entityMaps/{entityMapId} -- Retrieve an entity map (section 5.14.1).
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Path(entity_map_id): Path<String>,
) -> Result<Response, NgsiLdError> {
    let entity_map_id = parse_entity_map_id(&entity_map_id, &uri)?;
    let context: ContextSpec = context.into_inner();

    let map = state.entity_maps.retrieve(&entity_map_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

/// PATCH /ngsi-ld/v1/entityMaps/{entityMapId} -- Update an entity map (section 5.14.2).
pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(entity_map_id): Path<String>,
    Json(body): Json<UpdateEntityMapBody>,
) -> Result<Response, NgsiLdError> {
    let entity_map_id = parse_entity_map_id(&entity_map_id, &uri)?;

    let expires_at_str = body
        .expires_at
        .ok_or_else(|| AndromedaError::bad_request_data("expiresAt is required").with_instance(uri.clone()))?;

    let expires_at = DateTime::parse_from_rfc3339(&expires_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| AndromedaError::bad_request_data(format!("Invalid expiresAt datetime: {}", expires_at_str)).with_instance(uri.clone()))?;

    if expires_at <= chrono::Utc::now() {
        return Err(NgsiLdError::from(
            AndromedaError::bad_request_data("expiresAt must be in the future").with_instance(uri),
        ));
    }

    state.entity_maps.update(&entity_map_id, expires_at).await.map_err(|e| e.with_instance(uri))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /ngsi-ld/v1/entityMaps/{entityMapId} -- Delete an entity map (section 5.14.3).
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(entity_map_id): Path<String>) -> Result<Response, NgsiLdError> {
    let entity_map_id = parse_entity_map_id(&entity_map_id, &uri)?;
    state.entity_maps.delete(&entity_map_id).await.map_err(|e| e.with_instance(uri))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// GET /ngsi-ld/v1/entityMaps -- Create an entity map from query entities (section 5.14.4, section 6.34.3.1).
pub async fn create_from_query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    EntityQuery(query): EntityQuery,
) -> Result<Response, NgsiLdError> {
    // section 5.14.4.4: At least one filter must be provided for EntityMap creation
    validate_has_filter(&query, &uri)?;

    let context: ContextSpec = context.into_inner();

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let ids = state.entity_query.query_ids(&compiled).await.map_err(|e| e.with_instance(uri.clone()))?;

    let lifetime = query
        .entity_map_lifetime
        .as_deref()
        .map(|raw| parse_iso8601_duration(Some(raw)))
        .transpose()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let typed_ids: Vec<Uri> = ids
        .iter()
        .map(|id| Uri::from_str(id).map_err(|_| AndromedaError::bad_request_data(format!("Invalid entity id URI '{}'", id))))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let map = state
        .entity_maps
        .create(typed_ids, lifetime.as_ref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/entityMaps/{}", map.id);
    // Clone id before converting map to Value
    let map_uri = map.id.to_string();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// POST /ngsi-ld/v1/entityMaps -- Create an entity map from query body (section 5.14.4, section 6.34.3.2).
pub async fn create_from_query_post(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Json(body): Json<QueryRequestBody>,
) -> Result<Response, NgsiLdError> {
    let context_spec: ContextSpec = context.into_inner();

    let query = query_body_to_params(&body).map_err(|e| NgsiLdError::from(AndromedaError::bad_request_data(e.to_string())))?;
    query.validate().map_err(|e| NgsiLdError::from(AndromedaError::from(e)))?;
    validate_has_filter(&query, &uri)?;

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context_spec)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let ids = state.entity_query.query_ids(&compiled).await.map_err(|e| e.with_instance(uri.clone()))?;

    let lifetime = body
        .entity_map_lifetime
        .as_deref()
        .map(|raw| parse_iso8601_duration(Some(raw)))
        .transpose()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let typed_ids: Vec<Uri> = ids
        .iter()
        .map(|id| Uri::from_str(id).map_err(|_| AndromedaError::bad_request_data(format!("Invalid entity id URI '{}'", id))))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let map = state
        .entity_maps
        .create(typed_ids, lifetime.as_ref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/entityMaps/{}", map.id);
    let map_uri = map.id.to_string();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context_spec, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// GET /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query (section 5.14.5, section 6.35.3.1).
pub async fn create_temporal_from_query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    EntityQuery(query): EntityQuery,
) -> Result<Response, NgsiLdError> {
    validate_has_filter(&query, &uri)?;

    let context: ContextSpec = context.into_inner();

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let ids = state.entity_query.query_ids(&compiled).await.map_err(|e| e.with_instance(uri.clone()))?;

    let lifetime = query
        .entity_map_lifetime
        .as_deref()
        .map(|raw| parse_iso8601_duration(Some(raw)))
        .transpose()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let typed_ids: Vec<Uri> = ids
        .iter()
        .map(|id| Uri::from_str(id).map_err(|_| AndromedaError::bad_request_data(format!("Invalid entity id URI '{}'", id))))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let map = state
        .entity_maps
        .create(typed_ids, lifetime.as_ref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/temporal/entityMaps/{}", map.id);
    let map_uri = map.id.to_string();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// POST /ngsi-ld/v1/temporal/entityMaps -- Create an entity map from temporal query body (section 5.14.5, section 6.35.3.2).
pub async fn create_temporal_from_query_post(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Json(body): Json<QueryRequestBody>,
) -> Result<Response, NgsiLdError> {
    let context_spec: ContextSpec = context.into_inner();

    let query = query_body_to_params(&body).map_err(|e| NgsiLdError::from(AndromedaError::bad_request_data(e.to_string())))?;
    query.validate().map_err(|e| NgsiLdError::from(AndromedaError::from(e)))?;
    validate_has_filter(&query, &uri)?;

    let compiled = state
        .entity_query
        .compile_query_uncapped(&query, &context_spec)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;
    let ids = state.entity_query.query_ids(&compiled).await.map_err(|e| e.with_instance(uri.clone()))?;

    let lifetime = body
        .entity_map_lifetime
        .as_deref()
        .map(|raw| parse_iso8601_duration(Some(raw)))
        .transpose()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let typed_ids: Vec<Uri> = ids
        .iter()
        .map(|id| Uri::from_str(id).map_err(|_| AndromedaError::bad_request_data(format!("Invalid entity id URI '{}'", id))))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| NgsiLdError::from(e.with_instance(uri.clone())))?;
    let map = state
        .entity_maps
        .create(typed_ids, lifetime.as_ref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/temporal/entityMaps/{}", map.id);
    let map_uri = map.id.to_string();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context_spec, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}
