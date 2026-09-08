//! Handlers for the `/attributes` discovery endpoints.

use crate::{error::EntityResultExt, state::EntityTenantState};
use andromeda_entity::discovery::{AttributeCollectionRepresentation, DiscoveryScope};
use andromeda_federation::{
    discovery_result::{collect_discovery_warnings, merge_attribute_info_results, merge_attributes_results},
    operation::ForwardingOperation,
    request::{ForwardDiscoveryRequest, TriggeringRequest},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context::ContextUrl,
    context_target,
    prefer::PreferHeader,
    query::ValidatedQuery,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::{
    extract::OriginalUri,
    http::HeaderMap,
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/attributes -- Query available attributes.
pub async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    let is_local = !target.permits_federation() || params.local.unwrap_or(false);
    let scope = if target.permits_federation() {
        DiscoveryScope::for_local_request(params.local)
    } else {
        DiscoveryScope::Local
    };

    let representation = if params.details { AttributeCollectionRepresentation::Detailed } else { AttributeCollectionRepresentation::List };
    let local_result = service.attributes(&target, representation, scope, &context).await.with_entity_instance(&uri)?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_discovery_query(
                &target,
                ForwardDiscoveryRequest {
                    path: "/attributes",
                    query_string,
                    context: &context,
                    operation: if params.details {
                        ForwardingOperation::RetrieveAttrTypeDetails
                    } else {
                        ForwardingOperation::RetrieveAttrTypes
                    },
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                },
            )
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_attributes_results(local_result, forwarded, params.details);
        (merged, warnings)
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(result))
        .render()
        .with_entity_instance(&uri)?;
    let response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
    Ok(match snapshot {
        Some(snapshot_id) => response.with_snapshot_uri(snapshot_id).into_response(),
        None => response.into_response(),
    })
}

/// GET /ngsi-ld/v1/attributes/{attr_id} -- Retrieve information about a specific attribute.
pub async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    TypedPath(attr_id): TypedPath<AttributeName>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let context: ContextSpec = context.into_inner();

    let is_local = !target.permits_federation() || params.local.unwrap_or(false);
    let scope = if target.permits_federation() {
        DiscoveryScope::for_local_request(params.local)
    } else {
        DiscoveryScope::Local
    };

    let local_result = service.attribute_info(&target, &attr_id, scope, &context).await.with_entity_instance(&uri)?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let query_string = uri.query().unwrap_or("");
        let path = format!("/attributes/{attr_id}");
        let forwarded = state
            .forwarding
            .forward_discovery_query(
                &target,
                ForwardDiscoveryRequest {
                    path: &path,
                    query_string,
                    context: &context,
                    operation: ForwardingOperation::RetrieveAttrTypeInfo,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                },
            )
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_attribute_info_results(local_result, forwarded);
        (merged, warnings)
    };

    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(result))
        .render()
        .with_entity_instance(&uri)?;
    let response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
    Ok(match snapshot {
        Some(snapshot_id) => response.with_snapshot_uri(snapshot_id).into_response(),
        None => response.into_response(),
    })
}
