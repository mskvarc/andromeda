//! Handlers for the `/attributes` discovery endpoints.

use crate::{
    api::tenant::TenantState,
    core::error::ResultInstanceExt,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery, snapshot::SnapshotHeader, via::IncomingVia},
        response::{render::start_rendering, single::NgsiLdResponse},
        validation::validate_name_or_iri,
    },
    jsonld::ContextSpec,
    service::csource_forwarding::{collect_discovery_warnings, merge_attribute_info_results, merge_attributes_results},
};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::{
    extract::{OriginalUri, Path},
    response::{IntoResponse, Response},
};
use serde_json::json;

/// GET /ngsi-ld/v1/attributes -- Query available attributes.
pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    // ETSI GS CIM 009 clause 5.5.15.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let attributes = state.snapshots.snapshot_attributes(&snapshot_id).await.with_instance(&uri)?;
        let body = json!({
            "id": snapshot_id.as_str(),
            "type": "AttributeList",
            "attributeList": attributes,
        });
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(body))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).into_response());
    }

    let is_local = params.local.unwrap_or(false);

    let local_result = service.attributes(params.details, &context).await.with_instance(&uri)?;

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

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(result))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}

/// GET /ngsi-ld/v1/attributes/{attr_id} -- Retrieve information about a specific attribute.
pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    Path(attr_id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_discovery;
    validate_name_or_iri(&attr_id, &uri)?;
    let context: ContextSpec = context.into_inner();

    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let info = state.snapshots.snapshot_attribute_info(&snapshot_id, &attr_id).await.with_instance(&uri)?;
        let (attribute_count, type_names) = info.ok_or_else(|| {
            NgsiLdError::new(
                NgsiLdErrorType::ResourceNotFound,
                format!("Attribute '{attr_id}' not found in snapshot {}", snapshot_id.as_str()),
                uri.clone(),
            )
        })?;
        let body = json!({
            "id": format!("urn:ngsi-ld:Attribute:{attr_id}"),
            "type": "Attribute",
            "attributeName": attr_id,
            "attributeCount": attribute_count,
            "typeNames": type_names,
        });
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(body))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).into_response());
    }

    let is_local = params.local.unwrap_or(false);

    let local_result = service.attribute_info(&attr_id, &context).await.with_instance(&uri)?;

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

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(result))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
