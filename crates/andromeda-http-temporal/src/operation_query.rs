//! POST query handler for the NGSI-LD Temporal API.

use crate::{error::TemporalResultExt, join::apply_temporal_join, state::TemporalTenantState};
use andromeda_federation::{
    entity_result::collect_temporal_warnings,
    operation::ForwardingOperation,
    request::{CsrModeFilter, ForwardTemporalQueryRequest, TriggeringRequest},
    temporal_result::merge_temporal_query_results,
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    jsonld::JsonLd,
    prefer::PreferHeader,
    snapshot::SnapshotHeader,
    tenant_state,
    via::IncomingVia,
};
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    content_range::build_content_range,
    problem_response::ProblemResponse,
    render::start_rendering,
};
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{join_traversal::RetrievingJoin, query::temporal::TemporalQueryParameters};
use andromeda_temporal::service::TemporalQueryResult;
use axum::{
    extract::OriginalUri,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/temporal/entityOperations/query -- Query temporal entities via POST body.
pub async fn query(
    tenant_state::ExistingTenantState(state): TemporalTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.merge(body_context);

    let params = TemporalQueryParameters::try_from(request).with_instance(&uri)?;
    params.validate(true).with_instance(&uri)?;

    // ETSI GS CIM 009 clause 5.5.15.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate(&snapshot_id).await.with_instance(&uri)?;
        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let limit = i64::from(params.limit.unwrap_or(1000));
        let offset = i64::from(params.offset.unwrap_or(0));
        let (entities, _total) = state
            .snapshots
            .entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_instance(&uri)?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Collection(entities))
            .render()
            .with_instance(&uri)?;
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
        range,
        warnings: _,
        linked_candidates,
    } = result;

    let (mut entities, forwarding_warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_temporal_query(ForwardTemporalQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids_ref,
                query_string: "",
                context: &context,
                operation: ForwardingOperation::QueryTemporalEntity,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
                mode_filter: CsrModeFilter::All,
                entity_map: None,
            })
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        let merged = merge_temporal_query_results(local_entities, forwarded);
        (merged, warnings)
    };

    // Join mode (clause 4.5.23) arrives in the POST body exactly as it arrives
    // in a query string. `@none` and an absent `join` retrieve no linked entity
    // at all, so neither reaches the traversal.
    let forwarding_warnings = match RetrievingJoin::from_mode(params.join) {
        Some(join) => {
            apply_temporal_join(service, join, &mut entities, &linked_candidates, &params, &context, forwarding_warnings)
                .await
                .with_instance(&uri)?
                .warnings
        }
        None => forwarding_warnings,
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Collection(entities))
        .render()
        .with_instance(&uri)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(forwarding_warnings);
    if let Some(count) = total_count
        && let Ok(count) = usize::try_from(count)
    {
        response = response.with_total_count(count);
    }
    if any_truncated && range.is_some() {
        response = response.with_status(StatusCode::PARTIAL_CONTENT);
    }
    if let Some(range) = range
        && let Some(content_range) = build_content_range(&range.interval, range.observed, range.partition_total, range.last_n_origin)
    {
        response = response.with_content_range(content_range);
    }
    Ok(response.into_response())
}
