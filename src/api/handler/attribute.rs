use crate::{
    api::{
        extractor::{accept::AcceptFormat, context::ContextUrl, params::ValidatedQuery},
        response::ngsild::NgsiLdResponse,
        tenant::TenantState,
    },
    error::api::ApiError,
    jsonld::ContextSpec,
};
use axum::{
    extract::{OriginalUri, Path},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    #[serde(default)]
    pub details: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

pub async fn list(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    ValidatedQuery(params): ValidatedQuery<ListParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    let attrs = service.attributes(params.details, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(NgsiLdResponse::ok(attrs).with_context(context).with_format(accept.format()).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Path(attr_id): Path<String>,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    let attr_info = service.attribute_info(&attr_id, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    Ok(NgsiLdResponse::ok(attr_info).with_context(context).with_format(accept.format()).into_response())
}
