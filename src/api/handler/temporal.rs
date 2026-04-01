//! API handlers for the NGSI-LD Temporal API.

use crate::{
    api::{
        extractor::{accept::AcceptFormatNoGeo, context::ContextUrl, jsonld::JsonLd, params::ValidatedQuery},
        response::ngsild::{NgsiLdCollectionResponse, NgsiLdResponse},
        tenant::TenantState,
    },
    domain::entity::Entity,
    error::{AndromedaError, api::ApiError},
    jsonld::ContextSpec,
    query::temporal::TemporalQueryParameters,
    service::temporal::CreateOrUpdateResult,
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
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    let context: ContextSpec = context.into_inner();

    let result = service.get(id.as_ref(), &params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let options = params.options.as_deref().unwrap_or("");
    let key_values = options.contains("keyValues") || options.contains("simplified");

    let status = if result.content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    let content_range = result.content_range;
    let entity_value = result.value;

    if key_values {
        let entity: Entity = serde_json::from_value(entity_value.clone()).map_err(|e| {
            ApiError::from(AndromedaError::bad_request_data(format!(
                "Failed to parse temporal entity for simplification: {}",
                e
            )))
        })?;

        let mut response = NgsiLdResponse::new(status, entity.simplify())
            .with_context(context)
            .with_format(accept.format())
            .into_response();
        if let Some(cr) = content_range
            && let Ok(val) = cr.parse()
        {
            response.headers_mut().insert("Content-Range", val);
        }
        Ok(response)
    } else {
        let mut response = NgsiLdResponse::new(status, entity_value)
            .with_context(context)
            .with_format(accept.format())
            .into_response();
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
    ValidatedQuery(params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    let options = params.options.as_deref().unwrap_or("");
    let key_values = options.contains("keyValues") || options.contains("simplified");

    let result = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    if !result.warnings.is_empty() {
        tracing::debug!(count = result.warnings.len(), "Temporal query completed with warnings");
    }
    let crate::service::temporal::TemporalQueryResult {
        entities,
        total_count,
        any_truncated,
        content_range,
        ..
    } = result;

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

        let mut response = NgsiLdCollectionResponse::ok(simplified);
        response = response.with_context(context).with_format(accept.format());
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
        Ok(response.into_response())
    } else {
        let mut response = NgsiLdCollectionResponse::ok(entities);
        response = response.with_context(context).with_format(accept.format());
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
