//! POST query handler for the NGSI-LD Temporal API.

use crate::{error::TemporalResultExt, history_read::TemporalHistoryRead, join::apply_temporal_join, state::TemporalTenantState};
use andromeda_federation::{
    entity_result::collect_temporal_warnings,
    operation::ForwardingOperation,
    outcome::ForwardReadOutcome,
    request::{CsrModeFilter, EntityMapInUse, ForwardTemporalQueryRequest, TriggeringRequest},
    temporal_result::merge_temporal_query_results,
};
use andromeda_http_extraction::{accept::AcceptFormatNoGeo, context_target, jsonld::JsonLd, prefer::PreferHeader, request_envelope::NgsiLdRequestEnvelope};
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
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// POST /ngsi-ld/v1/temporal/entityOperations/query -- Query temporal entities via POST body.
pub async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TemporalTenantState,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    accept: AcceptFormatNoGeo,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.merge(body_context);

    let mut params = TemporalQueryParameters::try_from(request).with_instance(&uri)?;
    if !target.permits_federation() {
        params.local = Some(true);
    }
    params.validate(true).with_instance(&uri)?;

    let is_local = !target.permits_federation() || params.is_local();

    let result = service.query(&target, &params, &context).await.with_instance(&uri)?;
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
        let forwarded = state
            .forwarding
            .forward_temporal_query(
                &target,
                ForwardTemporalQueryRequest {
                    client_query: &params,
                    split: state.forwarding.split_entities(params.split_entities),
                    context: &context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                    csf_expr: None,
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                    source_page: None,
                },
            )
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "Context source forwarding failed during the temporal batch query");
                ForwardReadOutcome::empty()
            })
            .results;
        let warnings = collect_temporal_warnings(&forwarded);

        let merged = merge_temporal_query_results(local_entities, forwarded);
        (merged, warnings)
    };

    // Join mode (clause 4.5.23) arrives in the POST body exactly as it arrives
    // in a query string. `@none` and an absent `join` retrieve no linked entity
    // at all, so neither reaches the traversal.
    let forwarding_warnings = match RetrievingJoin::from_mode(params.join) {
        Some(join) => {
            let read = TemporalHistoryRead {
                service: &state.temporal,
                target: &target,
                params: &params,
                context: &context,
            };
            apply_temporal_join(&read, join, &mut entities, &linked_candidates, forwarding_warnings)
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
