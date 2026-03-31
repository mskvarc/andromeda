use crate::{
    api::{
        error::{
            ApiError,
            batch::{BatchEntityError, BatchProblemDetail},
            ngsi_ld::NgsiLdErrorType,
        },
        extractor::{accept::AcceptFormat, context::ContextUrl, jsonld::JsonLd, params::ValidatedQuery},
        response::ngsild::NgsiLdCollectionResponse,
        tenant::TenantState,
    },
    domain::{entity::Entity, registration::ForwardingMode},
    error::AndromedaError,
    jsonld::ContextSpec,
    query::entity::EntityQueryParameters,
    service::context_source_forwarding::ForwardBatchProvisionRequest,
};
use axum::{
    Json,
    body::Bytes,
    extract::OriginalUri,
    http::{HeaderMap, StatusCode, Uri, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use mediatype::{MediaTypeBuf, names};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    pub options: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    pub options: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub type_: Option<String>,
    pub entities: Option<Vec<EntitySelection>>,
    pub attrs: Option<Vec<String>>,
    pub q: Option<String>,
    pub geo_q: Option<GeoQueryRequest>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EntitySelection {
    /// Full object form: {"id": "...", "type": "...", "idPattern": "..."}
    Object {
        id: Option<String>,
        #[serde(rename = "idPattern")]
        id_pattern: Option<String>,
        #[serde(rename = "type")]
        type_: Option<String>,
    },
    /// Short form: just an entity ID string
    Id(String),
}

impl EntitySelection {
    pub fn id(&self) -> Option<&str> {
        match self {
            EntitySelection::Object { id, .. } => id.as_deref(),
            EntitySelection::Id(s) => Some(s.as_str()),
        }
    }

    pub fn id_pattern(&self) -> Option<&str> {
        match self {
            EntitySelection::Object { id_pattern, .. } => id_pattern.as_deref(),
            EntitySelection::Id(_) => None,
        }
    }

    pub fn type_(&self) -> Option<&str> {
        match self {
            EntitySelection::Object { type_, .. } => type_.as_deref(),
            EntitySelection::Id(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoQueryRequest {
    pub georel: String,
    pub geometry: String,
    pub coordinates: Value,
    pub geoproperty: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    pub options: Option<String>,
    pub format: Option<String>,
    pub lang: Option<String>,
    pub join: Option<String>,
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    pub pick: Option<String>,
    pub omit: Option<String>,
    pub local: Option<bool>,
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    ValidatedQuery(qparams): ValidatedQuery<PostQueryParams>,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<QueryRequest>,
) -> Result<Response, ApiError> {
    let service = &state.entity_query;
    let context = context.merge(body_context);

    // Map QueryRequest to EntityQueryParameters
    let mut ids = Vec::new();
    let mut types = Vec::new();
    let mut id_pattern = None;

    if let Some(entities) = request.entities {
        for selection in entities {
            if let Some(id) = selection.id() {
                ids.push(id.to_string());
            }
            if let Some(t) = selection.type_() {
                types.push(t.to_string());
            }
            if let Some(p) = selection.id_pattern() {
                id_pattern = Some(p.to_string());
            }
        }
    }

    let (georel, geometry, coordinates, geoproperty) = if let Some(geo) = request.geo_q {
        (Some(geo.georel), Some(geo.geometry), Some(geo.coordinates.to_string()), geo.geoproperty)
    } else {
        (None, None, None, None)
    };

    let params = EntityQueryParameters {
        id: if ids.is_empty() { None } else { Some(ids.join(",")) },
        entity_type: if types.is_empty() { None } else { Some(types.join(",")) },
        id_pattern,
        attrs: request.attrs.map(|a| a.join(",")),
        q: request.q,
        georel,
        geometry,
        coordinates,
        geoproperty,
        limit: request.limit,
        offset: request.offset,
        count: request.count,
        options: qparams.options,
        local: qparams.local,
        pick: qparams.pick,
        omit: qparams.omit,
        order_by: None,
        scope_q: None,
        lang: qparams.lang,
        dataset_id: qparams.dataset_id,
        expand_values: None,
        json_keys: None,
        format: qparams.format,
        join: qparams.join,
        join_level: qparams.join_level,
        contained_by: None,
        entity_map: None,
        entity_map_lifetime: None,
        split_entities: None,
        geometry_property: qparams.geometry_property,
        collation: None,
    };

    let opts = params.parse_options();
    let lang = params.lang.clone();
    let join_mode = params.join.clone();
    let join_level = params.join_level.unwrap_or(1);
    let geometry_property = params.geometry_property.clone();

    let is_local = params.local.unwrap_or(false);
    let (local_entities, total_count) = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    // Distributed forwarding: query matching context sources unless local=true
    let mut entities = if is_local {
        local_entities
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_query(
                entity_type,
                entity_ids_ref,
                context_url,
                &context,
                crate::service::context_source_forwarding::ForwardingOperation::QueryBatch,
            )
            .await
            .unwrap_or_default();
        for warning in crate::service::context_source_forwarding::collect_warnings(&forwarded) {
            tracing::debug!("Context source forwarding warning: {}", warning);
        }

        if forwarded.is_empty() {
            local_entities
        } else {
            let local_values: Vec<serde_json::Value> = local_entities.iter().map(|e| serde_json::to_value(e).unwrap_or_default()).collect();
            let merged_values = crate::service::context_source_forwarding::merge_query_results(local_values, forwarded);
            let mut result = Vec::new();
            for val in merged_values {
                match serde_json::from_value::<Entity>(val) {
                    Ok(e) => result.push(e),
                    Err(err) => {
                        tracing::debug!("Failed to deserialize merged entity: {}", err);
                    }
                }
            }
            result
        }
    };

    if let Some(ref lang) = lang {
        for entity in &mut entities {
            crate::response::entity::apply_language_negotiation(entity, lang);
        }
    }

    // Handle join
    if matches!(join_mode.as_deref(), Some("inline") | Some("flat")) {
        let linked = service
            .resolve_linked_entities(&entities, join_level, opts.sys_attrs, &context, &std::collections::HashSet::new())
            .await
            .map_err(|e| e.with_instance(uri.clone()))?;

        if join_mode.as_deref() == Some("flat") {
            let mut all_values: Vec<serde_json::Value> = Vec::new();
            for entity in &entities {
                if opts.key_values {
                    all_values.push(entity.simplify());
                } else {
                    all_values.push(serde_json::to_value(entity).unwrap_or_default());
                }
            }
            for linked_entity in linked.values() {
                if opts.key_values {
                    all_values.push(linked_entity.simplify());
                } else {
                    all_values.push(serde_json::to_value(linked_entity).unwrap_or_default());
                }
            }
            let mut response = NgsiLdCollectionResponse::ok(all_values);
            response = response
                .with_context(context)
                .with_format(accept.format())
                .with_geometry_property(geometry_property);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            return Ok(response.into_response());
        } else {
            // Inline: serialize normalized, apply join, then simplify if needed
            let mut entity_values: Vec<serde_json::Value> = Vec::new();
            for entity in &entities {
                let mut val = serde_json::to_value(entity).unwrap_or_default();
                crate::response::entity::apply_inline_join(&mut val, &linked);
                if opts.key_values {
                    val = crate::response::entity::simplify_value(&val);
                }
                entity_values.push(val);
            }
            let mut response = NgsiLdCollectionResponse::ok(entity_values);
            response = response
                .with_context(context)
                .with_format(accept.format())
                .with_geometry_property(geometry_property);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            return Ok(response.into_response());
        }
    }

    // Handle keyValues
    if opts.key_values {
        let simplified: Vec<serde_json::Value> = entities.iter().map(|e| e.simplify()).collect();
        let mut response = NgsiLdCollectionResponse::ok(simplified);
        response = response
            .with_context(context)
            .with_format(accept.format())
            .with_geometry_property(geometry_property);
        if let Some(count) = total_count {
            response = response.with_total_count(count as usize);
        }
        Ok(response.into_response())
    } else {
        let mut response = NgsiLdCollectionResponse::ok(entities);
        response = response
            .with_context(context)
            .with_format(accept.format())
            .with_geometry_property(geometry_property);
        if let Some(count) = total_count {
            response = response.with_total_count(count as usize);
        }
        Ok(response.into_response())
    }
}

/// Content-Type classification for batch entity parsing.
enum ContentKind {
    Json,
    LdJson,
}

fn classify_batch_content_type(raw: &str) -> Option<ContentKind> {
    let mt: MediaTypeBuf = raw.parse().ok()?;
    if mt.ty() != names::APPLICATION {
        return None;
    }
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::LdJson);
    }
    if mt.subty() == names::JSON && mt.suffix().is_none() {
        return Some(ContentKind::Json);
    }
    None
}

/// Parse a batch request body with per-entity error handling.
///
/// Returns `(valid_entities, context, per_entity_errors)`.
/// Whole-request errors (empty body, invalid JSON, unsupported Content-Type)
/// are returned as `Err`.
fn parse_batch_entities(
    body: &Bytes,
    headers: &HeaderMap,
    context: &ContextUrl,
    uri: &Uri,
) -> Result<(Vec<Entity>, ContextSpec, Vec<BatchEntityError>), ApiError> {
    let content_type_raw = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");

    let content_kind = classify_batch_content_type(content_type_raw).ok_or_else(|| {
        ApiError::from_error(
            AndromedaError::invalid_request(format!("Unsupported Media Type: {}", content_type_raw)),
            uri.clone(),
        )
    })?;

    let is_ld_json = matches!(content_kind, ContentKind::LdJson);
    let is_plain_json = matches!(content_kind, ContentKind::Json);

    // Check for conflicting Link header + ld+json
    if is_ld_json {
        let has_context_link = headers
            .get_all("Link")
            .iter()
            .any(|v| v.to_str().ok().map(|s| s.contains("http://www.w3.org/ns/json-ld#context")).unwrap_or(false));
        if has_context_link {
            return Err(ApiError::from_error(
                AndromedaError::bad_request_data("application/ld+json must not be combined with a Link context header"),
                uri.clone(),
            ));
        }
    }

    if body.is_empty() {
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Request body shall not be empty"),
            uri.clone(),
        ));
    }

    let value: Value =
        serde_json::from_slice(body).map_err(|e| ApiError::from_error(AndromedaError::invalid_request(format!("Invalid JSON: {}", e)), uri.clone()))?;

    let items = match value {
        Value::Array(arr) if arr.is_empty() => {
            return Err(ApiError::from_error(
                AndromedaError::bad_request_data("Request body must contain a non-empty array of entities"),
                uri.clone(),
            ));
        }
        Value::Array(arr) => arr,
        _ => {
            return Err(ApiError::from_error(
                AndromedaError::bad_request_data("Request body must be a JSON array of entities"),
                uri.clone(),
            ));
        }
    };

    // Merge all @context values from all items for the overall context
    let mut merged_context = context.0.clone();
    if is_ld_json {
        let mut seen = std::collections::HashSet::new();
        let mut merged_ctx_values = Vec::new();
        for item in &items {
            if let Some(ctx) = item.as_object().and_then(|o| o.get("@context")) {
                match ctx {
                    Value::Array(arr) => {
                        for v in arr {
                            let key = serde_json::to_string(v).unwrap_or_default();
                            if seen.insert(key) {
                                merged_ctx_values.push(v.clone());
                            }
                        }
                    }
                    other => {
                        let key = serde_json::to_string(other).unwrap_or_default();
                        if seen.insert(key) {
                            merged_ctx_values.push(other.clone());
                        }
                    }
                }
            }
        }
        if !merged_ctx_values.is_empty() {
            let ctx_value = if merged_ctx_values.len() == 1 {
                merged_ctx_values.into_iter().next().unwrap()
            } else {
                Value::Array(merged_ctx_values)
            };
            merged_context = merged_context.with_body_context(Some(ctx_value));
        }
    }

    let mut entities = Vec::new();
    let mut parse_errors = Vec::new();

    for mut item in items {
        let entity_id = item
            .as_object()
            .and_then(|o| o.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Per-entity @context validation
        let has_context = item.as_object().is_some_and(|o| o.contains_key("@context"));

        if is_plain_json && has_context {
            parse_errors.push(make_batch_parse_error(
                &entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/json body must not contain @context",
            ));
            continue;
        }

        if is_ld_json && !has_context {
            parse_errors.push(make_batch_parse_error(
                &entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/ld+json body must contain @context",
            ));
            continue;
        }

        // Strip @context before deserializing
        if let Some(obj) = item.as_object_mut() {
            obj.remove("@context");
        }

        match serde_json::from_value::<Entity>(item) {
            Ok(entity) => entities.push(entity),
            Err(e) => {
                parse_errors.push(make_batch_parse_error(
                    &entity_id,
                    NgsiLdErrorType::BadRequestData,
                    &format!("Invalid entity: {}", e),
                ));
            }
        }
    }

    Ok((entities, merged_context, parse_errors))
}

fn make_batch_parse_error(entity_id: &str, error_type: NgsiLdErrorType, detail: &str) -> BatchEntityError {
    BatchEntityError {
        entity_id: entity_id.to_string(),
        error: BatchProblemDetail {
            type_: error_type.type_uri().to_string(),
            title: error_type.title().to_string(),
            status: error_type.status_code().as_u16(),
            detail: detail.to_string(),
        },
    }
}

fn has_supported_json_content_type(headers: &HeaderMap) -> bool {
    classify_batch_content_type(headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("")).is_some()
}

pub async fn batch_create(
    crate::api::tenant::TenantStateOrCreate(state): crate::api::tenant::TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !has_supported_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let service = &state.entities;
    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Request body must contain a non-empty array of entities"),
            uri.clone(),
        ));
    }

    // Distributed forwarding: forward to matching context sources
    // Per spec 5.6.7: iterate CSRs, forward matching entities
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        crate::domain::entity::EntityTypeValue::Single(s) => s.as_str(),
        crate::domain::entity::EntityTypeValue::Multiple(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
    });
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: crate::service::context_source_forwarding::ForwardingOperation::CreateBatch,
            batch_path: "/entityOperations/create",
            query_string: None,
        })
        .await
        .unwrap_or_default();

    let has_exclusive = forwarded.iter().any(|r| r.mode == ForwardingMode::Exclusive);
    let has_redirect = forwarded.iter().any(|r| r.mode == ForwardingMode::Redirect);

    if has_exclusive || has_redirect {
        // Per spec 4.3.6.3: exclusive/redirect — don't create locally, return CS result
        let first = forwarded.first().unwrap();
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::CREATED);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: create locally
    let mut result = service.batch_create(&entities, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok((StatusCode::CREATED, Json(result.success)).into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}

pub async fn batch_upsert(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<UpsertParams>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !has_supported_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let service = &state.entities;
    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Request body must contain a non-empty array of entities"),
            uri.clone(),
        ));
    }

    let update_mode = params.options.as_deref() == Some("update");

    // Distributed forwarding: forward to matching context sources
    // Per spec 5.6.8: iterate CSRs, forward matching entities
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        crate::domain::entity::EntityTypeValue::Single(s) => s.as_str(),
        crate::domain::entity::EntityTypeValue::Multiple(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
    });
    let qs = params.options.as_deref().map(|o| format!("options={o}"));
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: crate::service::context_source_forwarding::ForwardingOperation::UpsertBatch,
            batch_path: "/entityOperations/upsert",
            query_string: qs.as_deref(),
        })
        .await
        .unwrap_or_default();

    let has_exclusive = forwarded.iter().any(|r| r.mode == ForwardingMode::Exclusive);
    let has_redirect = forwarded.iter().any(|r| r.mode == ForwardingMode::Redirect);

    if has_exclusive || has_redirect {
        let first = forwarded.first().unwrap();
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: upsert locally
    let (mut result, created_ids) = service
        .batch_upsert(&entities, &context, update_mode)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        if created_ids.is_empty() {
            Ok(StatusCode::NO_CONTENT.into_response())
        } else {
            Ok((StatusCode::CREATED, Json(created_ids)).into_response())
        }
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}

pub async fn batch_update(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<UpdateParams>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !has_supported_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let service = &state.entities;
    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Request body must contain a non-empty array of entities"),
            uri.clone(),
        ));
    }

    let no_overwrite = params.options.as_deref().map(|o| o.contains("noOverwrite")).unwrap_or(false);

    // Distributed forwarding per spec 5.6.9
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        crate::domain::entity::EntityTypeValue::Single(s) => s.as_str(),
        crate::domain::entity::EntityTypeValue::Multiple(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
    });
    let qs = params.options.as_deref().map(|o| format!("options={o}"));
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: crate::service::context_source_forwarding::ForwardingOperation::UpdateBatch,
            batch_path: "/entityOperations/update",
            query_string: qs.as_deref(),
        })
        .await
        .unwrap_or_default();

    let has_exclusive = forwarded.iter().any(|r| r.mode == ForwardingMode::Exclusive);
    let has_redirect = forwarded.iter().any(|r| r.mode == ForwardingMode::Redirect);

    if has_exclusive || has_redirect {
        let first = forwarded.first().unwrap();
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let mut result = service
        .batch_update(&entities, &context, no_overwrite)
        .await
        .map_err(|e| e.with_instance(uri.clone()))?;

    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}

pub async fn batch_merge(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if !has_supported_json_content_type(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let service = &state.entities;
    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Request body must contain a non-empty array of entities"),
            uri.clone(),
        ));
    }

    // Distributed forwarding per spec 5.6.20
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_type = entities.first().map(|e| match &e.type_ {
        crate::domain::entity::EntityTypeValue::Single(s) => s.as_str(),
        crate::domain::entity::EntityTypeValue::Multiple(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
    });
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: crate::service::context_source_forwarding::ForwardingOperation::MergeBatch,
            batch_path: "/entityOperations/merge",
            query_string: None,
        })
        .await
        .unwrap_or_default();

    let has_exclusive = forwarded.iter().any(|r| r.mode == ForwardingMode::Exclusive);
    let has_redirect = forwarded.iter().any(|r| r.mode == ForwardingMode::Redirect);

    if has_exclusive || has_redirect {
        let first = forwarded.first().unwrap();
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let mut result = service.batch_merge(&entities, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}

pub async fn batch_delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, context: ContextUrl, body: Bytes) -> Result<Response, ApiError> {
    let service = &state.entities;
    let entity_ids: Vec<String> = serde_json::from_slice(&body).map_err(|e| {
        ApiError::from_error(
            AndromedaError::invalid_request(format!("Request body must be a JSON array of entity IDs: {}", e)),
            uri.clone(),
        )
    })?;

    if entity_ids.is_empty() {
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Request body must contain a non-empty array of entity IDs"),
            uri.clone(),
        ));
    }

    // Distributed forwarding per spec 5.6.10
    let context: crate::jsonld::ContextSpec = context.into_inner();
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: None, // delete doesn't have entity type in payload
            body: &body,
            context_url,
            context: &context,
            operation: crate::service::context_source_forwarding::ForwardingOperation::DeleteBatch,
            batch_path: "/entityOperations/delete",
            query_string: None,
        })
        .await
        .unwrap_or_default();

    let has_exclusive = forwarded.iter().any(|r| r.mode == ForwardingMode::Exclusive);
    let has_redirect = forwarded.iter().any(|r| r.mode == ForwardingMode::Redirect);

    if has_redirect {
        // Redirect: don't delete locally, return CS result
        let first = forwarded.first().unwrap();
        let status = StatusCode::from_u16(first.status).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(ref resp_body) = first.body {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    if has_exclusive {
        // Exclusive: delete locally AND forwarded (already done)
        let _ = service.batch_delete(&entity_ids).await;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Inclusive or no match: delete locally
    let result = service.batch_delete(&entity_ids).await.map_err(|e| e.with_instance(uri.clone()))?;

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}
