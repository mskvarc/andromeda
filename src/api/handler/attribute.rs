use crate::{
    api::tenant::TenantState,
    error::api::ApiError,
    http::{
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery},
        header::via::Via,
        parameter::{DiscoveryListParams, LocalOnlyParams},
        response::single::NgsiLdResponse,
    },
    jsonld::ContextSpec,
    renderer::response::ResponseRenderer,
    service::context_source_forwarding::{collect_discovery_warnings, merge_attribute_info_results, merge_attributes_results},
};
use axum::{
    extract::{OriginalUri, Path},
    http::HeaderMap,
    response::{IntoResponse, Response},
};

pub async fn list(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    headers: HeaderMap,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    let local_result = service.attributes(params.details, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

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
            .forward_discovery_query("/attributes", query_string, context_url, incoming_via.as_ref())
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_attributes_results(local_result, forwarded, params.details);
        (merged, warnings)
    };

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result);
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    headers: HeaderMap,
    Path(attr_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, ApiError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();
    let is_local = params.local.unwrap_or(false);

    let local_result = service.attribute_info(&attr_id, &context).await.map_err(|e| e.with_instance(uri.clone()))?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let incoming_via: Option<Via> = {
            let mut via_values = headers.get_all("via").iter();
            headers::Header::decode(&mut via_values).ok()
        };
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

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(result);
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
