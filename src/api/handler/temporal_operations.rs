//! POST query handler for the NGSI-LD Temporal API.

use crate::{
    api::tenant::TenantState,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, jsonld::JsonLd, prefer::PreferHeader, via::IncomingVia},
        model::temporal_query_request::TemporalQueryRequest,
        response::collection::NgsiLdCollectionResponse,
    },
    query::temporal::TemporalQueryParameters,
    renderer::response::ResponseRenderer,
    service::{
        context_source_forwarding::{ForwardingOperation, collect_warnings},
        temporal::TemporalService,
    },
};
use axum::{
    extract::OriginalUri,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/temporal/entityOperations/query
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<TemporalQueryRequest>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context = context.merge(body_context);

    // Convert POST query body to TemporalQueryParameters using From impl
    let params: TemporalQueryParameters = request.into();

    let is_local = params.is_local();

    let result = service.query(&params, &context).await.map_err(|e| e.with_instance(uri.clone()))?;
    for warning in &result.warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal batch query warning");
    }
    let crate::service::temporal::TemporalQueryResult {
        entities: local_entities,
        total_count,
        any_truncated,
        content_range,
        warnings: _,
    } = result;

    let (entities, forwarding_warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_temporal_query(&crate::service::context_source_forwarding::ForwardTemporalQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids_ref,
                query_string: "",
                context_url,
                context: &context,
                operation: ForwardingOperation::QueryTemporalEntity,
                incoming_via: incoming_via.as_ref(),
            })
            .await
            .unwrap_or_default();
        let warnings = collect_warnings(&forwarded);

        let merged = TemporalService::merge_with_forwarded(local_entities, forwarded);
        (merged, warnings)
    };

    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_collection(entities)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(forwarding_warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    if any_truncated {
        response = response.with_status(axum::http::StatusCode::PARTIAL_CONTENT);
    }
    if let Some(cr) = content_range {
        response = response.with_content_range(cr);
    }
    Ok(response.into_response())
}
