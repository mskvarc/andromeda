//! Handlers for the `/attributes` discovery endpoints.

use crate::{
    api::tenant::TenantState,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery, via::IncomingVia},
        parameter::{DiscoveryListParams, LocalOnlyParams},
        response::single::NgsiLdResponse,
        validation::validate_name_or_iri,
    },
    jsonld::ContextSpec,
    renderer::response::ResponseRenderer,
    service::context_source_forwarding::{collect_discovery_warnings, merge_attribute_info_results, merge_attributes_results},
};
use axum::{
    extract::{OriginalUri, Path},
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/attributes -- Query available attributes.
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

    let local_result = service.attributes(params.details, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_discovery_query("/attributes", query_string, context_url, incoming_via.as_ref())
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_attributes_results(local_result, forwarded, params.details);
        (merged, warnings)
    };

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result)?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}

/// GET /ngsi-ld/v1/attributes/{attr_id} -- Retrieve information about a specific attribute.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    Path(attr_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_discovery;
    validate_name_or_iri(&attr_id, &uri)?;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    let local_result = service.attribute_info(&attr_id, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        let path = format!("/attributes/{}", attr_id);
        let forwarded = state
            .forwarding
            .forward_discovery_query(&path, query_string, context_url, incoming_via.as_ref())
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_attribute_info_results(local_result, forwarded);
        (merged, warnings)
    };

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result)?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
