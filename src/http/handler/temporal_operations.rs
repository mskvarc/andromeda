//! POST query handler for the NGSI-LD Temporal API.

use crate::{
    api::tenant::TenantState,
    core::error::ResultInstanceExt,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, jsonld::JsonLd, prefer::PreferHeader, snapshot::SnapshotHeader, via::IncomingVia},
        response::{collection::NgsiLdCollectionResponse, render::start_rendering},
    },
    service::{
        csource_forwarding::{CsrModeFilter, ForwardTemporalQueryRequest, ForwardingOperation, collect_temporal_warnings},
        temporal::{TemporalQueryResult, TemporalService},
    },
};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{body::temporal_query::TemporalQueryRequest, query::temporal::TemporalQueryParameters};
use axum::{
    extract::OriginalUri,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/temporal/entityOperations/query -- Query temporal entities via POST body.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<TemporalQueryRequest>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context = context.merge(body_context);

    // Convert POST query body to TemporalQueryParameters using From impl
    let params: TemporalQueryParameters = request.into();

    // ETSI GS CIM 009 clause 5.5.15.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let limit = i64::from(params.limit.unwrap_or(1000));
        let offset = i64::from(params.offset.unwrap_or(0));
        let (entities, _total) = state
            .snapshots
            .query_snapshot_temporal_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_instance(&uri)?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Collection(entities))
            .render()?;
        return Ok(NgsiLdCollectionResponse::ok(rendered).into_response());
    }

    let is_local = params.is_local();

    let result = service.query(&params, &context).await.with_instance(&uri)?;
    for warning in &result.warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal batch query warning");
    }
    let TemporalQueryResult {
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
            .forward_temporal_query(
                &ForwardTemporalQueryRequest {
                    entity_type_compact: entity_type,
                    entity_ids: entity_ids_ref,
                    query_string: "",
                    context_url,
                    context: &context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    incoming_via: incoming_via.as_ref(),
                },
                CsrModeFilter::All,
            )
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        let merged = TemporalService::merge_with_forwarded(local_entities, forwarded);
        (merged, warnings)
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Collection(entities))
        .render()?;
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
