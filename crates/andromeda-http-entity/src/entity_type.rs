//! Handlers for entity-type discovery through the `/types` endpoints.

use crate::{error::EntityResultExt, state::EntityTenantState};
use andromeda_entity::discovery::DiscoveryScope;
use andromeda_federation::{
    discovery_result::{collect_discovery_warnings, merge_type_info_results, merge_types_results},
    operation::ForwardingOperation,
    request::{ForwardDiscoveryRequest, TriggeringRequest},
};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    context_target,
    prefer::PreferHeader,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    typed_path::TypedPath,
};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering_normalized, single::NgsiLdResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::response::{IntoResponse, Response};

/// GET /ngsi-ld/v1/types -- Query available entity types.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when local type discovery fails, when the
/// type list cannot be serialised, when a referenced context cannot be
/// resolved, or when the discovery result cannot be rendered under the
/// negotiated media type.
pub async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot,
    }: EntityTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
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

    if params.details {
        let local_result = service.type_details(&target, scope, &context).await.with_entity_instance(&uri)?;

        let (result, warnings) = if is_local {
            (local_result, Vec::new())
        } else {
            let query_string = uri.query().unwrap_or("");
            let forwarded = state
                .forwarding
                .forward_discovery_query(
                    &target,
                    ForwardDiscoveryRequest {
                        path: "/types",
                        query_string,
                        context: &context,
                        operation: ForwardingOperation::RetrieveEntityTypeDetails,
                        triggering: TriggeringRequest {
                            via: incoming_via.as_ref(),
                            headers: Some(&headers),
                        },
                    },
                )
                .await;
            let warnings = collect_discovery_warnings(&forwarded);
            let merged = merge_types_results(local_result, forwarded, true);
            (merged, warnings)
        };

        let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Single(result))
            .render()
            .with_entity_instance(&uri)?;
        let response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
        Ok(match snapshot {
            Some(snapshot_id) => response.with_snapshot_uri(snapshot_id).into_response(),
            None => response.into_response(),
        })
    } else {
        let types = service.types(&target, scope, &context).await.with_entity_instance(&uri)?;
        let local_result = serde_json::to_value(&types).with_entity_instance(&uri)?;

        let (result, warnings) = if is_local {
            (local_result, Vec::new())
        } else {
            let query_string = uri.query().unwrap_or("");
            let forwarded = state
                .forwarding
                .forward_discovery_query(
                    &target,
                    ForwardDiscoveryRequest {
                        path: "/types",
                        query_string,
                        context: &context,
                        operation: ForwardingOperation::RetrieveEntityTypes,
                        triggering: TriggeringRequest {
                            via: incoming_via.as_ref(),
                            headers: Some(&headers),
                        },
                    },
                )
                .await;
            let warnings = collect_discovery_warnings(&forwarded);
            let merged = merge_types_results(local_result, forwarded, false);
            (merged, warnings)
        };

        let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Single(result))
            .render()
            .with_entity_instance(&uri)?;
        let response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
        Ok(match snapshot {
            Some(snapshot_id) => response.with_snapshot_uri(snapshot_id).into_response(),
            None => response.into_response(),
        })
    }
}

/// GET /ngsi-ld/v1/types/{type_} -- Retrieve information about a specific entity type.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the named entity type is unknown to the
/// broker, when a referenced context cannot be resolved, or when the discovery
/// result cannot be rendered under the negotiated media type.
pub async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot,
    }: EntityTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    TypedPath(type_): TypedPath<EntityTypeName>,
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

    let local_result = service.type_info(&target, &type_, scope, &context).await.with_entity_instance(&uri)?;

    let (result, warnings) = if is_local {
        (local_result, Vec::new())
    } else {
        let query_string = uri.query().unwrap_or("");
        let path = format!("/types/{type_}");
        let forwarded = state
            .forwarding
            .forward_discovery_query(
                &target,
                ForwardDiscoveryRequest {
                    path: &path,
                    query_string,
                    context: &context,
                    operation: ForwardingOperation::RetrieveEntityTypeInfo,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                },
            )
            .await;
        let warnings = collect_discovery_warnings(&forwarded);
        let merged = merge_type_info_results(local_result, forwarded);
        (merged, warnings)
    };

    let rendered = start_rendering_normalized(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(result))
        .render()
        .with_entity_instance(&uri)?;
    let response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
    Ok(match snapshot {
        Some(snapshot_id) => response.with_snapshot_uri(snapshot_id).into_response(),
        None => response.into_response(),
    })
}
