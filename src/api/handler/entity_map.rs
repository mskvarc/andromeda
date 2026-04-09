use crate::{
    api::tenant::TenantState,
    error::AndromedaError,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, entity_query::EntityQuery, prefer::PreferHeader},
        response::single::NgsiLdResponse,
    },
    jsonld::ContextSpec,
    renderer::response::ResponseRenderer,
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::DateTime;
use serde::Deserialize;
use serde_json::Value;

fn parse_entity_map_id(id: &str, uri: &axum::http::Uri) -> Result<(), NgsiLdError> {
    if id.is_empty() || (!id.starts_with("urn:") && url::Url::parse(id).is_err()) {
        return Err(NgsiLdError::from(
            AndromedaError::bad_request_data("Invalid entityMap id: not a valid URI").with_instance(uri.clone()),
        ));
    }
    Ok(())
}

/// GET /ngsi-ld/v1/entityMaps/{entityMapId} -- Retrieve EntityMap (section 5.14.1)
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Path(entity_map_id): Path<String>,
) -> Result<Response, NgsiLdError> {
    parse_entity_map_id(&entity_map_id, &uri)?;
    let context: ContextSpec = context.into_inner();

    let map = state.entity_maps.retrieve(&entity_map_id).await.map_err(|e| e.with_instance(uri.clone()))?;

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}

#[derive(Debug, Deserialize)]
pub struct UpdateEntityMapBody {
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    // Output-only fields -- accepted but ignored (section 5.14.2.4)
    #[serde(rename = "entityMap")]
    #[allow(dead_code)]
    pub entity_map: Option<Value>,
    #[serde(rename = "linkedMaps")]
    #[allow(dead_code)]
    pub linked_maps: Option<Value>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub type_: Option<String>,
    #[allow(dead_code)]
    pub id: Option<String>,
}

/// PATCH /ngsi-ld/v1/entityMaps/{entityMapId} -- Update EntityMap (section 5.14.2)
pub async fn update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(entity_map_id): Path<String>,
    Json(body): Json<UpdateEntityMapBody>,
) -> Result<Response, NgsiLdError> {
    parse_entity_map_id(&entity_map_id, &uri)?;

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

/// DELETE /ngsi-ld/v1/entityMaps/{entityMapId} -- Delete EntityMap (section 5.14.3)
pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(entity_map_id): Path<String>) -> Result<Response, NgsiLdError> {
    state.entity_maps.delete(&entity_map_id).await.map_err(|e| e.with_instance(uri))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// GET /ngsi-ld/v1/entityMaps -- Create EntityMap from Query Entities (section 5.14.4, section 6.34.3.1)
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

    let map = state
        .entity_maps
        .create(ids, query.entity_map_lifetime.as_deref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/entityMaps/{}", map.id);
    // Clone id before converting map to Value
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// POST /ngsi-ld/v1/entityMaps -- Create EntityMap from Query body (section 5.14.4, section 6.34.3.2)
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

    let lifetime = body.entity_map_lifetime.as_deref();
    let map = state.entity_maps.create(ids, lifetime).await.map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/entityMaps/{}", map.id);
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context_spec, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// GET /ngsi-ld/v1/temporal/entityMaps -- Create EntityMap from Temporal Query (section 5.14.5, section 6.35.3.1)
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

    let map = state
        .entity_maps
        .create(ids, query.entity_map_lifetime.as_deref())
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/temporal/entityMaps/{}", map.id);
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

/// POST /ngsi-ld/v1/temporal/entityMaps -- Create EntityMap from Temporal Query body (section 5.14.5, section 6.35.3.2)
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

    let lifetime = body.entity_map_lifetime.as_deref();
    let map = state.entity_maps.create(ids, lifetime).await.map_err(|e| e.with_instance(uri.clone()))?;

    let location = format!("/ngsi-ld/v1/temporal/entityMaps/{}", map.id);
    let map_uri = map.id.clone();

    let value = serde_json::to_value(&map).map_err(|e| AndromedaError::internal("Failed to serialize entity map").with_source(e).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context_spec, PreferHeader::default()).render_single(value)?;

    Ok(NgsiLdResponse::new(StatusCode::CREATED, rendered)
        .with_location(location)
        .with_entity_map_uri(map_uri)
        .into_response())
}

// -- Query body support --

#[derive(Debug, Deserialize)]
pub struct QueryRequestBody {
    pub entities: Option<Vec<EntitySelector>>,
    pub attrs: Option<Vec<String>>,
    pub q: Option<String>,
    pub georel: Option<String>,
    pub geometry: Option<String>,
    pub coordinates: Option<String>,
    pub geoproperty: Option<String>,
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntitySelector {
    pub id: Option<Vec<String>>,
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

fn query_body_to_params(body: &QueryRequestBody) -> Result<crate::query::entity::EntityQueryParameters, serde_urlencoded::de::Error> {
    let (id, entity_type, id_pattern) = if let Some(entities) = &body.entities {
        let ids: Vec<String> = entities.iter().filter_map(|e| e.id.clone()).flatten().collect();
        let types: Vec<String> = entities.iter().filter_map(|e| e.type_.clone()).collect();
        (
            if ids.is_empty() { None } else { Some(ids.join(",")) },
            if types.is_empty() { None } else { Some(types.join(",")) },
            entities.iter().filter_map(|e| e.id_pattern.clone()).next(),
        )
    } else {
        (None, None, None)
    };

    // Use serde_urlencoded to construct parameters matching EntityQueryParameters
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(ref id) = id {
        pairs.push(("id".to_string(), id.clone()));
    }
    if let Some(ref t) = entity_type {
        pairs.push(("type".to_string(), t.clone()));
    }
    if let Some(ref p) = id_pattern {
        pairs.push(("idPattern".to_string(), p.clone()));
    }
    if let Some(ref attrs) = body.attrs {
        pairs.push(("attrs".to_string(), attrs.join(",")));
    }
    if let Some(ref q) = body.q {
        pairs.push(("q".to_string(), q.clone()));
    }
    if let Some(ref georel) = body.georel {
        pairs.push(("georel".to_string(), georel.clone()));
    }
    if let Some(ref geometry) = body.geometry {
        pairs.push(("geometry".to_string(), geometry.clone()));
    }
    if let Some(ref coordinates) = body.coordinates {
        pairs.push(("coordinates".to_string(), coordinates.clone()));
    }
    if let Some(ref geoproperty) = body.geoproperty {
        pairs.push(("geoproperty".to_string(), geoproperty.clone()));
    }
    if let Some(ref scope_q) = body.scope_q {
        pairs.push(("scopeQ".to_string(), scope_q.clone()));
    }
    // Always add local=true since entity map creation is always local
    pairs.push(("local".to_string(), "true".to_string()));

    let qs = serde_urlencoded::to_string(&pairs).unwrap_or_default();
    serde_urlencoded::from_str(&qs).or_else(|_| {
        // Fallback: construct with defaults via minimal query string
        serde_urlencoded::from_str("local=true")
    })
}

fn validate_has_filter(query: &crate::query::entity::EntityQueryParameters, uri: &axum::http::Uri) -> Result<(), NgsiLdError> {
    let has_filter = query.entity_type.is_some()
        || query.id.is_some()
        || query.id_pattern.is_some()
        || query.attrs.is_some()
        || query.georel.is_some()
        || query.scope_q.is_some()
        || query.q.as_ref().is_some_and(|q| !q.is_empty());

    if !has_filter {
        return Err(NgsiLdError::from(
            AndromedaError::bad_request_data("At least one query filter must be provided (type, attrs, q, georel, scopeQ, or id)").with_instance(uri.clone()),
        ));
    }

    Ok(())
}
