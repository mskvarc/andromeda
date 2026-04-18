use crate::{
    api::tenant::TenantState,
    core::error::{AndromedaError, ErrorKind, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{context::ContextUrl, protocol::NgsiLdProtocolHeaders, query::ValidatedQuery},
        parameter::{
            i18n::{LocalizedEntityGetParams, LocalizedEntityQueryParams},
            protocol::{apply_entity_get_extension_headers, apply_entity_query_extension_headers},
        },
        validation::parse_entity_id,
    },
    i18n::validation::validate_locale,
    jsonld::ContextSpec,
    renderer::{Renderer, SysAttrsInclusion, localization::LocalizationRenderer},
};
use axum::{
    Json,
    extract::{OriginalUri, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::collections::HashSet;

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalizedEntityGetParams>,
) -> Result<Response, NgsiLdError> {
    reject_ld_json_accept(&headers, &uri)?;
    validate_locale(&params.locale).with_instance(&uri)?;
    validate_optional_token(params.calendar.as_deref(), "calendar").with_instance(&uri)?;
    validate_optional_token(params.numbering_system.as_deref(), "numberingSystem").with_instance(&uri)?;
    let locale = params.locale.clone();
    let localized_attrs = localized_attr_filter(params.localized_attrs.as_deref());

    let mut base = params.into_get_params();
    apply_entity_get_extension_headers(&mut base, &protocol, &uri)?;

    let service = &state.entity_query;
    let id = parse_entity_id(&id, &uri)?;
    let context: ContextSpec = context.into_inner();
    let attrs: Vec<String> = base
        .attrs
        .as_deref()
        .map(|raw| raw.split(',').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect())
        .unwrap_or_default();
    let dataset_ids: Vec<String> = base
        .dataset_id
        .as_deref()
        .map(|raw| raw.split(',').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect())
        .unwrap_or_default();

    let entity = service
        .get_inner(&id, &attrs, SysAttrsInclusion::Strip, &dataset_ids, &context)
        .await
        .with_instance(&uri)?;

    let value = serde_json::to_value(entity).map_err(|error| AndromedaError::internal(format!("Failed to serialize entity: {}", error)))?;
    let localized = LocalizationRenderer::new(value, &locale).with_attrs_filter(localized_attrs.as_ref()).render()?;
    Ok((StatusCode::OK, Json(localized)).into_response())
}

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(params): ValidatedQuery<LocalizedEntityQueryParams>,
) -> Result<Response, NgsiLdError> {
    reject_ld_json_accept(&headers, &uri)?;
    validate_locale(&params.locale).with_instance(&uri)?;
    validate_optional_token(params.calendar.as_deref(), "calendar").with_instance(&uri)?;
    validate_optional_token(params.numbering_system.as_deref(), "numberingSystem").with_instance(&uri)?;

    let locale = params.locale.clone();
    let localized_attrs = localized_attr_filter(params.localized_attrs.as_deref());
    let mut query = params.into_entity_query();
    apply_entity_query_extension_headers(&mut query, &protocol, &uri)?;
    query.validate().with_instance(&uri)?;

    let service = &state.entity_query;
    let context: ContextSpec = context.into_inner();
    let (entities, total_count) = service.query(&query, &context).await.with_instance(&uri)?;
    let localized_values: Vec<Value> = entities
        .into_iter()
        .map(|entity| {
            let value = serde_json::to_value(entity).unwrap_or_default();
            LocalizationRenderer::new(value, &locale).with_attrs_filter(localized_attrs.as_ref()).render()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut result = serde_json::Map::new();
    result.insert("results".to_string(), Value::Array(localized_values));
    if let Some(count) = total_count {
        result.insert("count".to_string(), Value::Number(count.into()));
    }
    Ok((StatusCode::OK, Json(Value::Object(result))).into_response())
}

fn reject_ld_json_accept(headers: &HeaderMap, uri: &axum::http::Uri) -> Result<(), NgsiLdError> {
    let Some(accept_value) = headers.get(axum::http::header::ACCEPT).and_then(|value| value.to_str().ok()) else {
        return Ok(());
    };
    if accept_value.contains("application/ld+json") {
        return Err(NgsiLdError::from(
            AndromedaError::from(ErrorKind::OperationNotSupported("Localized endpoints only return application/json".to_string())).with_instance(uri),
        ));
    }
    Ok(())
}

fn localized_attr_filter(raw: Option<&str>) -> Option<HashSet<String>> {
    raw.map(|value| value.split(',').map(str::trim).filter(|attr| !attr.is_empty()).map(ToOwned::to_owned).collect())
}

fn validate_optional_token(value: Option<&str>, name: &str) -> Result<(), AndromedaError> {
    let Some(value) = value else {
        return Ok(());
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AndromedaError::malformed_request(format!("Invalid {} value: empty string", name)));
    }
    if !trimmed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err(AndromedaError::malformed_request(format!("Invalid {} value: '{}'", name, trimmed)));
    }
    Ok(())
}
