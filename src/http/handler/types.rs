//! Handlers for the `/types` discovery endpoints.

use crate::{
    api::tenant::TenantState,
    core::error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader, query::ValidatedQuery, snapshot::SnapshotHeader, via::IncomingVia},
        response::{render::start_rendering, single::NgsiLdResponse},
        validation::validate_name_or_iri,
    },
    service::csource_forwarding::{collect_discovery_warnings, merge_type_info_results, merge_types_results},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::{
    extract::{OriginalUri, Path},
    response::{IntoResponse, Response},
};
use serde_json::json;

/// GET /ngsi-ld/v1/types -- Query available entity types.
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

    // ETSI GS CIM 009 clause 5.5.15: snapshot scoping applies to the Core API.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let types = state.snapshots.snapshot_types(&snapshot_id).await.with_instance(&uri)?;
        let body = json!({
            "id": snapshot_id.as_str(),
            "type": "EntityTypeList",
            "typeList": types,
        });
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(body))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).into_response());
    }

    let is_local = params.local.unwrap_or(false);

    if params.details {
        let local_result = service.type_details(&context).await.with_instance(&uri)?;

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

        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(result))
            .render()?;
        Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
    } else {
        let types = service.types(&context).await.with_instance(&uri)?;
        let local_result =
            serde_json::to_value(&types).map_err(|e| AndromedaError::internal(format!("Failed to serialize types for merge: {}", e)).with_instance(&uri))?;

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

        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(result))
            .render()?;
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
    SnapshotHeader(snapshot_id): SnapshotHeader,
    Path(type_): Path<String>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_discovery;
    validate_name_or_iri(&type_, &uri)?;
    let context: ContextSpec = context.into_inner();

    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let info = state.snapshots.snapshot_type_info(&snapshot_id, &type_).await.with_instance(&uri)?;
        let (entity_count, attribute_names) = info.ok_or_else(|| {
            NgsiLdError::new(
                NgsiLdErrorType::ResourceNotFound,
                format!("Type '{type_}' not found in snapshot {}", snapshot_id.as_str()),
                uri.clone(),
            )
        })?;
        let body = json!({
            "id": format!("urn:ngsi-ld:EntityType:{type_}"),
            "type": "EntityType",
            "typeName": type_,
            "entityCount": entity_count,
            "attributeNames": attribute_names,
        });
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Single(body))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).into_response());
    }

    let is_local = params.local.unwrap_or(false);

    let local_result = service.type_info(&type_, &context).await.with_instance(&uri)?;

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

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(result))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
