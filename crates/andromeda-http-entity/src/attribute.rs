//! Handlers for the `/attributes` discovery endpoints.

use crate::{
    error::{EntityResultExt, public_error},
    state::EntityTenantState,
};
use andromeda_entity::discovery::AttributeCollectionRepresentation;
use andromeda_federation::discovery_result::{collect_discovery_warnings, merge_attribute_info_results, merge_attributes_results};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    prefer::PreferHeader,
    query::ValidatedQuery,
    snapshot::SnapshotHeader,
    tenant_state,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::{
    extract::OriginalUri,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// GET /ngsi-ld/v1/attributes -- Query available attributes.
pub async fn query(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    // ETSI GS CIM 009 clause 5.5.15.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate(&snapshot_id).await.with_entity_instance(&uri)?;
        let attributes = state.snapshots.attributes(&snapshot_id).await.with_entity_instance(&uri)?;
        let body = json!({
            "id": snapshot_id.as_str(),
            "type": "AttributeList",
            "attributeList": attributes,
        });
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Single(body))
            .render()
            .with_entity_instance(&uri)?;
        return Ok(NgsiLdResponse::ok(rendered).into_response());
    }

    let is_local = params.local.unwrap_or(false);

    let representation = match params.details {
        true => AttributeCollectionRepresentation::Detailed,
        false => AttributeCollectionRepresentation::List,
    };
    let local_result = service.attributes(representation, &context).await.with_entity_instance(&uri)?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let context_url = context.link_contexts().first();
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_discovery_query("/attributes", query_string, context_url, incoming_via.as_ref())
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_attributes_results(local_result, forwarded, params.details);
        (merged, warnings)
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(result))
        .render()
        .with_entity_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}

/// GET /ngsi-ld/v1/attributes/{attr_id} -- Retrieve information about a specific attribute.
pub async fn get(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    TypedPath(attr_id): TypedPath<AttributeName>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate(&snapshot_id).await.with_entity_instance(&uri)?;
        let info = state.snapshots.attribute_info(&snapshot_id, &attr_id).await.with_entity_instance(&uri)?;
        let (attribute_count, type_names) = info.ok_or_else(|| {
            public_error(
                NgsiLdErrorType::ResourceNotFound,
                format!("Attribute '{attr_id}' not found in snapshot {}", snapshot_id.as_str()),
                &uri,
            )
        })?;
        let body = json!({
            "id": format!("urn:ngsi-ld:Attribute:{attr_id}"),
            "type": "Attribute",
            "attributeName": attr_id,
            "attributeCount": attribute_count,
            "typeNames": type_names,
        });
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Single(body))
            .render()
            .with_entity_instance(&uri)?;
        return Ok(NgsiLdResponse::ok(rendered).into_response());
    }

    let is_local = params.local.unwrap_or(false);

    let local_result = service.attribute_info(&attr_id, &context).await.with_entity_instance(&uri)?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let context_url = context.link_contexts().first();
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

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(result))
        .render()
        .with_entity_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
