//! POST query handler for the NGSI-LD Temporal API.

use crate::{
    api::{
        extractor::{accept::AcceptFormat, context::ContextUrl, jsonld::JsonLd},
        handler::entity_operations::{EntitySelection, GeoQueryRequest},
        response::ngsild::NgsiLdCollectionResponse,
        tenant::TenantState,
    },
    error::api::ApiError,
    query::temporal::TemporalQueryParameters,
    service::context_source_forwarding::{ForwardingOperation, collect_warnings, merge_temporal_query_results},
};
use axum::{
    extract::OriginalUri,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalQueryRequest {
    #[serde(rename = "type")]
    pub _type: String,
    pub entities: Option<Vec<EntitySelection>>,
    pub attrs: Option<Vec<String>>,
    pub q: Option<String>,
    pub geo_q: Option<GeoQueryRequest>,
    pub temporal_q: Option<TemporalQ>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
    pub format: Option<String>,
    pub last_n: Option<u32>,
    pub local: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalQ {
    pub timerel: String,
    pub time_at: String,
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,
    pub aggr_methods: Option<String>,
    pub aggr_period_duration: Option<String>,
}

/// POST /ngsi-ld/v1/temporal/entityOperations/query
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    headers: HeaderMap,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<TemporalQueryRequest>,
) -> Result<Response, ApiError> {
    let service = &state.temporal;
    let context = context.merge(body_context);

    let mut ids = Vec::new();
    let mut types = Vec::new();
    let mut id_pattern = None;

    if let Some(entities) = request.entities {
        for sel in entities {
            if let Some(id) = sel.id() {
                ids.push(id.to_string());
            }
            if let Some(t) = sel.type_() {
                types.push(t.to_string());
            }
            if let Some(p) = sel.id_pattern() {
                id_pattern = Some(p.to_string());
            }
        }
    }

    let (timerel, time_at, end_time_at, timeproperty, aggr_methods, aggr_period_duration) = if let Some(tq) = request.temporal_q {
        (
            Some(tq.timerel),
            Some(tq.time_at),
            tq.end_time_at,
            tq.timeproperty,
            tq.aggr_methods,
            tq.aggr_period_duration,
        )
    } else {
        (None, None, None, None, None, None)
    };

    let params = TemporalQueryParameters {
        id: if ids.is_empty() { None } else { Some(ids.join(",")) },
        entity_type: if types.is_empty() { None } else { Some(types.join(",")) },
        id_pattern,
        attrs: request.attrs.map(|a: Vec<String>| a.join(",")),
        q: request.q,
        georel: request.geo_q.as_ref().map(|g| g.georel.clone()),
        geometry: request.geo_q.as_ref().map(|g| g.geometry.clone()),
        coordinates: request.geo_q.as_ref().map(|g| g.coordinates.to_string()),
        geoproperty: request.geo_q.as_ref().and_then(|g| g.geoproperty.clone()),
        timerel,
        time_at,
        end_time_at,
        timeproperty,
        last_n: request.last_n,
        format: request.format,
        options: None,
        aggr_methods,
        aggr_period_duration,
        limit: request.limit,
        offset: request.offset,
        count: request.count,
        _order_by: None,
        pick: None,
        omit: None,
        dataset_id: None,
        _lang: None,
        local: request.local,
        _scope_q: None,
        expand_values: None,
        json_keys: None,
    };

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
        let incoming_via = headers.get("via").and_then(|v| v.to_str().ok());
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let entity_type = params.entity_type.as_deref();
        let entity_ids: Option<Vec<String>> = params.id.as_deref().map(|s| s.split(',').map(|id| id.trim().to_string()).collect());
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_temporal_query(
                entity_type,
                entity_ids_ref,
                "",
                context_url,
                &context,
                ForwardingOperation::QueryTemporalEntity,
                incoming_via,
            )
            .await
            .unwrap_or_default();
        let warnings = collect_warnings(&forwarded);

        if forwarded.is_empty() {
            (local_entities, warnings)
        } else {
            let merged = merge_temporal_query_results(local_entities, forwarded);
            (merged, warnings)
        }
    };

    let mut response = NgsiLdCollectionResponse::ok(entities);
    response = response.with_context(context).with_format(accept.format()).with_warnings(forwarding_warnings);
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
