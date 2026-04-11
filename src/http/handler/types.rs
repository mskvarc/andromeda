//! Handlers for the `/types` discovery endpoints.

use crate::{
    api::tenant::TenantState,
    core::error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery, via::IncomingVia},
        parameter::{DiscoveryListParams, LocalOnlyParams},
        response::single::NgsiLdResponse,
        validation::validate_name_or_iri,
    },
    jsonld::ContextSpec,
    renderer::response::ResponseRenderer,
    service::context_source_forwarding::{collect_discovery_warnings, merge_type_info_results, merge_types_results},
};
use axum::{
    extract::{OriginalUri, Path},
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/types -- Query available entity types.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    if params.details {
        let local_result = service.type_details(&context).await.with_instance(uri.clone())?;

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

        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result)?;
        Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
    } else {
        let types = service.types(&context).await.with_instance(uri.clone())?;
        let local_result = serde_json::to_value(&types)
            .map_err(|e| AndromedaError::internal(format!("Failed to serialize types for merge: {}", e)).with_instance(uri.clone()))?;

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

        let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result)?;
        Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
    }
}

/// GET /ngsi-ld/v1/types/{type_} -- Retrieve information about a specific entity type.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    Path(type_): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_discovery;
    validate_name_or_iri(&type_, &uri)?;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    let local_result = service.type_info(&type_, &context).await.with_instance(uri.clone())?;

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

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result)?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
