//! HTTP handling for temporal delete attribute instance (clause 5.6.15).

use crate::temporal::{
    AttributeName,
    ContextSpec,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardingOperation,
    HeaderMap,
    IncomingVia,
    InstanceFamily,
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
    Uri,
    ValidatedQuery,
    collect_proxied_attrs,
    local_temporal_outcome,
    provision_response,
};
use andromeda_http_extraction::tenant_state;

pub async fn delete_attr_instance(
    tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id, instance_id)): TypedPath<(Uri, AttributeName, Uri)>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    let path_suffix = format!("/attrs/{attr_id}/{instance_id}");
    // Clause 4.5.7 carries an `instanceId` as a URI; which of the two stored
    // shapes it names is decided here, at the edge, so the store addresses one
    // identity column rather than searching both.
    let instance = InstanceIdentity::from_uri(instance_id, InstanceFamily::Attribute);

    if params.local.unwrap_or(false) {
        service.delete_attr_instance(&id, &attr_id, &instance, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            resource: ProvisionResource::TemporalEntities,
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: Some(&attr_id),
            method: Method::DELETE,
            body: None,
            context: &context,
            operation: ForwardingOperation::DeleteTemporalAttributeInstance,
            path_suffix: Some(&path_suffix),
            query_string: None,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
        })
        .await
        .with_instance(&uri)?;

    // Clause 5.6.15.4: an instance of an attribute a proxied registration
    // covers is deleted at the source and needs no further processing.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        local_temporal_outcome(service.delete_attr_instance(&id, &attr_id, &instance, &context).await, &forwarded)
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
