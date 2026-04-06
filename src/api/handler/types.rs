use crate::{
    api::{response::ngsild::NgsiLdResponse, tenant::TenantState},
    error::{AndromedaError, api::ApiError},
    http::{
        extractor::{accept::AcceptFormat, context::ContextUrl, query::ValidatedQuery},
        header::via::Via,
    },
    jsonld::ContextSpec,
    service::context_source_forwarding::{collect_discovery_warnings, merge_type_info_results, merge_types_results},
};
use axum::{
    extract::{OriginalUri, Path},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer};

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    #[serde(default)]
    pub details: bool,
    #[serde(default, deserialize_with = "deserialize_bool_case_insensitive")]
    pub local: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetTypeParams {
    #[serde(default, deserialize_with = "deserialize_bool_case_insensitive")]
    pub local: Option<bool>,
}

pub async fn list(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    headers: HeaderMap,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    if params.details {
        let local_result = service.type_details(&context).await.map_err(|e| e.with_instance(uri.clone()))?;

        let (result, warnings) = if is_local {
            (local_result, Vec::new())
        } else {
            let incoming_via: Option<Via> = {
                let mut via_values = headers.get_all("via").iter();
                headers::Header::decode(&mut via_values).ok()
            };
            let context_url = context.link_contexts().first().map(|s| s.as_str());
            let query_string = uri.query().unwrap_or("");
            let forwarded = state
                .forwarding
                .forward_discovery_query("/types", query_string, context_url, incoming_via.as_ref())
                .await;
            let warnings = collect_discovery_warnings(&forwarded);
            let merged = merge_types_results(local_result, forwarded, true);
            (merged, warnings)
        };

        Ok(NgsiLdResponse::ok(result)
            .with_context(context)
            .with_format(accept.format())
            .with_warnings(warnings)
            .into_response())
    } else {
        let types = service.types(&context).await.map_err(|e| e.with_instance(uri.clone()))?;
        let local_result = serde_json::to_value(&types)
            .map_err(|e| ApiError::from_error(AndromedaError::internal("Failed to serialize types for merge").with_source(e), uri.clone()))?;

        let (result, warnings) = if is_local {
            (local_result, Vec::new())
        } else {
            let incoming_via: Option<Via> = {
                let mut via_values = headers.get_all("via").iter();
                headers::Header::decode(&mut via_values).ok()
            };
            let context_url = context.link_contexts().first().map(|s| s.as_str());
            let query_string = uri.query().unwrap_or("");
            let forwarded = state
                .forwarding
                .forward_discovery_query("/types", query_string, context_url, incoming_via.as_ref())
                .await;
            let warnings = collect_discovery_warnings(&forwarded);
            let merged = merge_types_results(local_result, forwarded, false);
            (merged, warnings)
        };

        Ok(NgsiLdResponse::ok(result)
            .with_context(context)
            .with_format(accept.format())
            .with_warnings(warnings)
            .into_response())
    }
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    headers: HeaderMap,
    Path(type_): Path<String>,
    ValidatedQuery(params): ValidatedQuery<GetTypeParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    let local_result = service.type_info(&type_, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let incoming_via: Option<Via> = {
            let mut via_values = headers.get_all("via").iter();
            headers::Header::decode(&mut via_values).ok()
        };
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        let path = format!("/types/{}", type_);
        let forwarded = state
            .forwarding
            .forward_discovery_query(&path, query_string, context_url, incoming_via.as_ref())
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_type_info_results(local_result, forwarded);
        (merged, warnings)
    };

    Ok(NgsiLdResponse::ok(result)
        .with_context(context)
        .with_format(accept.format())
        .with_warnings(warnings)
        .into_response())
}
