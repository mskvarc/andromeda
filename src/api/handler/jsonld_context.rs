use crate::{
    api::tenant::TenantState,
    domain::jsonld_context::ContextKind,
    error::{AndromedaError, api::ApiError},
    http::extractor::query::ValidatedQuery,
};
use axum::{
    Json,
    body::Bytes,
    extract::{OriginalUri, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

fn deserialize_case_insensitive_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(serde::de::Error::custom(format!("Invalid boolean value: {}", s))),
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "deserialize_case_insensitive_bool")]
    pub details: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeParams {
    #[serde(default, deserialize_with = "deserialize_case_insensitive_bool")]
    pub details: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    #[serde(default, deserialize_with = "deserialize_case_insensitive_bool")]
    pub reload: Option<bool>,
}

pub async fn create(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    let service = &state.contexts;
    let payload: Value =
        serde_json::from_slice(&body).map_err(|_| ApiError::from_error(AndromedaError::invalid_request("Request body is not valid JSON"), uri.clone()))?;

    let base_url = headers.get("host").and_then(|v| v.to_str().ok()).map(|h| format!("http://{}", h));

    let id = service.create(payload, base_url.as_deref()).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/jsonldContexts/{}", id))]).into_response())
}

pub async fn list(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.contexts;
    let kind = params
        .kind
        .map(|k| {
            k.parse::<ContextKind>()
                .map_err(|_| crate::error::AndromedaError::bad_request_data(format!("Invalid kind: {}. Must be one of: Cached, Hosted, ImplicitlyCreated", k)))
        })
        .transpose()
        .map_err(|e| e.with_instance(uri.clone()))?;

    let details = params.details.unwrap_or(false);

    let result = service.list(kind, details).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(context_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<ServeParams>,
) -> Result<Response, ApiError> {
    let service = &state.contexts;
    let details = params.details.unwrap_or(false);

    let result = service.get(&context_id, details).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// Check if the URL refers to an NGSI-LD core context
fn is_core_context(url: &str) -> bool {
    url.contains("uri.etsi.org/ngsi-ld") && url.contains("core-context")
}

pub async fn delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    Path(context_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<DeleteParams>,
) -> Result<Response, ApiError> {
    let service = &state.contexts;
    let reload = params.reload.unwrap_or(false);

    // Check if attempting to delete a core context
    if is_core_context(&context_id) {
        if reload {
            // Core contexts are embedded; reload is a no-op success.
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        return Err(ApiError::from_error(
            AndromedaError::bad_request_data("Cannot delete a core context"),
            uri.clone(),
        ));
    }

    service.delete(&context_id, reload).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
