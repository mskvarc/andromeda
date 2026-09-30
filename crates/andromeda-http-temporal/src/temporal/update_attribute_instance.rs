//! HTTP handling for temporal update attribute instance (clause 5.6.14).

use crate::{
    temporal::{
        AttributeName,
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
        InstanceFamily,
        InstanceIdentity,
        IntoResponse,
        JsonLdDocument,
        LocalOnlyParams,
        LocalOutcome,
        Method,
        NgsiLdRequestEnvelope,
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
    },
    written_evolution_admission::admit_written_instance,
};
use andromeda_http_extraction::context_target;

pub async fn update_attr_instance(
    // Clause 6.3.20: the parameters are read first, before the tenant is
    // resolved, so an undefined one is an `InvalidRequest` on any tenant.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath((id, attr_id, instance_id)): TypedPath<(Identifier, AttributeName, Identifier)>,
    JsonLdDocument { document: payload, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let path_suffix = format!("/attrs/{attr_id}/{instance_id}");
    // Clause 4.5.7 carries an `instanceId` as a URI; which of the two stored
    // shapes it names is decided here, at the edge, so the store addresses one
    // identity column rather than searching both.
    let instance = InstanceIdentity::from_identifier(instance_id, InstanceFamily::Attribute);
    // Clause 5.6.14.3: the body is an EntityTemporal Fragment naming one
    // Attribute with an Array of exactly one instance, screened before it is
    // forwarded, so a Context Source receives that shape and nothing else. The
    // seals clauses 4.5.3.2 and 4.5.3.3 prohibit, and a user `@context` clause
    // 5.5.7 refuses below the top level, are held to it as to any written
    // Temporal Evolution.
    admit_written_instance(service.engine(), &id, &payload, &context, &uri).await?;

    if params.local.unwrap_or(false) {
        service
            .modify_attr_instance(&target, &id, &attr_id, &instance, payload, &context)
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
                entity_type_compact: None,
                attribute_name: Some(&attr_id),
                method: Method::PATCH,
                body: Some(&payload),
                context: &context,
                operation: ForwardingOperation::UpdateTemporalAttributeInstance,
                path_suffix: Some(&path_suffix),
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_instance(&uri)?;

    // Clause 5.6.14.4: an instance of an attribute a proxied registration
    // covers is modified at the source and needs no further processing.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        local_temporal_outcome(
            service.modify_attr_instance(&target, &id, &attr_id, &instance, payload, &context).await,
            &forwarded,
        )
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
