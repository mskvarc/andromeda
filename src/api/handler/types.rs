use crate::{
    api::tenant::TenantState,
    error::{AndromedaError, api::ApiError},
    http::{
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery, via::IncomingVia},
        parameter::{DiscoveryListParams, LocalOnlyParams},
        response::single::NgsiLdResponse,
    },
    jsonld::ContextSpec,
    renderer::response::ResponseRenderer,
    service::context_source_forwarding::{collect_discovery_warnings, merge_type_info_results, merge_types_results},
};
use axum::{
    extract::{OriginalUri, Path},
    response::{IntoResponse, Response},
};

pub async fn list(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    if params.details {
        let local_result = service.type_details(&context).await.map_err(|e| e.with_instance(uri.clone()))?;

        let (result, warnings) = if is_local {
            (local_result, Vec::new())
        } else {
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

        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result);
        Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
    } else {
        let types = service.types(&context).await.map_err(|e| e.with_instance(uri.clone()))?;
        let local_result = serde_json::to_value(&types)
            .map_err(|e| ApiError::from_error(AndromedaError::internal("Failed to serialize types for merge").with_source(e), uri.clone()))?;

        let (result, warnings) = if is_local {
            (local_result, Vec::new())
        } else {
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

        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result);
        Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
    }
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    Path(type_): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    let local_result = service.type_info(&type_, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
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

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result);
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
