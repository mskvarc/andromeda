//! Handlers for the `/attributes` discovery endpoints.

use crate::{
    discovered_response::{DiscoveryRequest, discovered_response},
    error::EntityResultExt,
    state::EntityTenantState,
};
use andromeda_entity::discovery::{AttributeCollectionRepresentation, DiscoveryScope, attribute_list::AttributeCollection};
use andromeda_federation::{discovery_result::Discovered, operation::ForwardingOperation, request::DiscoveryTarget};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    context::ContextUrl,
    context_target,
    path_segment::AttrId,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    typed_path::TypedPath,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::response::Response;

/// GET /ngsi-ld/v1/attributes -- Query available attributes.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when local attribute discovery fails, when a
/// referenced context cannot be resolved, or when the discovery result cannot
/// be rendered under the negotiated media type.
pub async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot,
    }: EntityTenantState,
    accept: AcceptFormatNoGeo,
    envelope: NgsiLdRequestEnvelope,
    ContextUrl(context): ContextUrl,
    api_root: ApiRoot,
    ValidatedQuery(params): ValidatedQuery<DiscoveryListParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let scope = if target.permits_federation() {
        DiscoveryScope::for_local_request(params.local)
    } else {
        DiscoveryScope::Local
    };
    let (representation, operation) = if params.details {
        (AttributeCollectionRepresentation::Detailed, ForwardingOperation::RetrieveAttrTypeDetails)
    } else {
        (AttributeCollectionRepresentation::List, ForwardingOperation::RetrieveAttrTypes)
    };
    let local = match service
        .attributes(&target, representation, scope, &context)
        .await
        .with_entity_instance(&envelope.uri)?
    {
        AttributeCollection::List(list) => Discovered::Attributes(list),
        AttributeCollection::Detailed(attributes) => Discovered::AttributeDetails(attributes),
    };
    discovered_response(DiscoveryRequest {
        state: &state,
        target: &target,
        snapshot,
        accept,
        envelope: &envelope,
        context: &context,
        api_root: &api_root,
        local_only: params.local.unwrap_or(false),
        discovered: DiscoveryTarget::Attributes,
        operation,
        local,
    })
    .await
}

/// GET /ngsi-ld/v1/attributes/{attr_id} -- Retrieve information about a specific attribute.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the named attribute is unknown to the
/// broker, when a referenced context cannot be resolved, or when the discovery
/// result cannot be rendered under the negotiated media type.
pub async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot,
    }: EntityTenantState,
    accept: AcceptFormatNoGeo,
    envelope: NgsiLdRequestEnvelope,
    ContextUrl(context): ContextUrl,
    api_root: ApiRoot,
    TypedPath(AttrId(attr_id)): TypedPath<AttrId>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let scope = if target.permits_federation() {
        DiscoveryScope::for_local_request(params.local)
    } else {
        DiscoveryScope::Local
    };
    let local = service
        .attribute_info(&target, &attr_id, scope, &context)
        .await
        .with_entity_instance(&envelope.uri)?;
    discovered_response(DiscoveryRequest {
        state: &state,
        target: &target,
        snapshot,
        accept,
        envelope: &envelope,
        context: &context,
        api_root: &api_root,
        local_only: params.local.unwrap_or(false),
        discovered: DiscoveryTarget::Attribute(&attr_id),
        operation: ForwardingOperation::RetrieveAttrTypeInfo,
        local: Discovered::AttributeInfo(local),
    })
    .await
}
