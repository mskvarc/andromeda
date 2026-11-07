//! HTTP handling for temporal delete attribute instance (clause 5.6.15).

use crate::temporal::{
    ContextSpec,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardingOperation,
    HeaderMap,
    IncomingVia,
    InstanceIdentity,
    IntoResponse,
    LocalOnlyParams,
    LocalOutcome,
    Method,
    OriginalUri,
    ProblemResponse,
    ProvisionResource,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TriggeringRequest,
    TypedPath,
    ValidatedQuery,
    collect_proxied_attrs,
    local_temporal_outcome,
    provision_response,
};
use andromeda_http_extraction::{
    context_target,
    path_segment::{AttrId, EntityId, InstanceId},
};

/// Delete Attribute instance from Temporal Evolution (clause 5.6.15): 204 when
/// every part succeeded, 207 with the clause 5.2.16 result when only some did.
pub(crate) async fn delete_attr_instance(
    // Clause 6.3.20: the parameters are read first, before the tenant is
    // resolved, so an undefined one is an `InvalidRequest` on any tenant.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((EntityId(id), AttrId(attr_id), InstanceId(instance_id))): TypedPath<(EntityId, AttrId, InstanceId)>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    let path_suffix = format!("/attrs/{attr_id}/{instance_id}");
    // Clause 4.5.7 carries an `instanceId` as a URI; which of the two stored
    // shapes it names is decided here, at the edge, so the store addresses one
    // identity column rather than searching both.
    let instance = InstanceIdentity::from_identifier(instance_id);

    if params.local.unwrap_or(false) {
        service
            .delete_attr_instance(&target, &id, &attr_id, &instance, &context)
            .await
            .with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::TemporalEntities,
                entity_id: &id,
                entity_type_selector: None,
                attribute_name: Some(&attr_id),
                method: Method::DELETE,
                context: &context,
                operation: ForwardingOperation::DeleteTemporalAttributeInstance,
                path_suffix: Some(&path_suffix),
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            None,
        )
        .await
        .with_instance(&uri)?;

    // Clause 5.6.15.4: an instance of an attribute a proxied registration
    // covers is deleted at the source and needs no further processing.
    let local = if collect_proxied_attrs(&forwarded).is_empty() {
        local_temporal_outcome(service.delete_attr_instance(&target, &id, &attr_id, &instance, &context).await, &forwarded)
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
