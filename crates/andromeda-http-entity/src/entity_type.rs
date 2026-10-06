//! Handlers for entity-type discovery through the `/types` endpoints.

use crate::{
    discovered_response::{DiscoveryRequest, discovered_response},
    error::EntityResultExt,
    state::EntityTenantState,
};
use andromeda_entity::discovery::DiscoveryScope;
use andromeda_federation::{discovery_result::Discovered, operation::ForwardingOperation, request::DiscoveryTarget};
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    context::ContextUrl,
    context_target,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    typed_path::TypedPath,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_request::{discovery::DiscoveryListParams, local::LocalOnlyParams};
use axum::response::Response;

/// GET /ngsi-ld/v1/types -- Query available entity types.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when local type discovery fails, when a
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
    let (local, operation) = if params.details {
        (
            Discovered::TypeDetails(service.type_details(&target, scope, &context).await.with_entity_instance(&envelope.uri)?),
            ForwardingOperation::RetrieveEntityTypeDetails,
        )
    } else {
        (
            Discovered::Types(service.types(&target, scope, &context).await.with_entity_instance(&envelope.uri)?),
            ForwardingOperation::RetrieveEntityTypes,
        )
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
        discovered: DiscoveryTarget::Types,
        operation,
        local,
    })
    .await
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
    envelope: NgsiLdRequestEnvelope,
    ContextUrl(context): ContextUrl,
    api_root: ApiRoot,
    TypedPath(type_): TypedPath<EntityTypeName>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_discovery;
    let scope = if target.permits_federation() {
        DiscoveryScope::for_local_request(params.local)
    } else {
        DiscoveryScope::Local
    };
    let local = service.type_info(&target, &type_, scope, &context).await.with_entity_instance(&envelope.uri)?;
    discovered_response(DiscoveryRequest {
        state: &state,
        target: &target,
        snapshot,
        accept,
        envelope: &envelope,
        context: &context,
        api_root: &api_root,
        local_only: params.local.unwrap_or(false),
        discovered: DiscoveryTarget::Type(&type_),
        operation: ForwardingOperation::RetrieveEntityTypeInfo,
        local: Discovered::TypeInfo(local),
    })
    .await
}
