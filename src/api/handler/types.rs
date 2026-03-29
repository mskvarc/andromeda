use crate::{
    api::{
        error::ApiError,
        extractor::{accept::AcceptFormat, context::ContextUrl, params::ValidatedQuery},
        response::ngsild::NgsiLdResponse,
        tenant::TenantState,
    },
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

    if params.details {
        let type_details = service.type_details(&context).await.map_err(|e| e.with_instance(uri.clone()))?;

        Ok(NgsiLdResponse::ok(type_details)
            .with_context(context)
            .with_format(accept.format())
            .into_response())
    } else {
        let types = service.types(&context).await.map_err(|e| e.with_instance(uri.clone()))?;

        Ok(NgsiLdResponse::ok(types).with_context(context).with_format(accept.format()).into_response())
    }
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Path(type_): Path<String>,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    let type_info = service.type_info(&type_, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    // type_info is already compacted by the service
    Ok(NgsiLdResponse::ok(type_info).with_context(context).with_format(accept.format()).into_response())
}
