//! HTTP handling for temporal update attribute instance (clause 5.6.14).

use crate::temporal::{
    AttributeName,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardingOperation,
    HeaderMap,
    IncomingVia,
    InstanceFamily,
    InstanceIdentity,
    IntoResponse,
    JsonLd,
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
    Value,
    collect_proxied_attrs,
    local_temporal_outcome,
    provision_response,
};
use andromeda_http_extraction::tenant_state;
use andromeda_http_response::classified_report::ClassifiedReport;

pub async fn update_attr_instance(
    tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id, instance_id)): TypedPath<(Uri, AttributeName, Uri)>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    let path_suffix = format!("/attrs/{attr_id}/{instance_id}");
    // Clause 4.5.7 carries an `instanceId` as a URI; which of the two stored
    // shapes it names is decided here, at the edge, so the store addresses one
    // identity column rather than searching both.
    let instance = InstanceIdentity::from_uri(instance_id, InstanceFamily::Attribute);

    if params.local.unwrap_or(false) {
        service
            .modify_attr_instance(&id, &attr_id, &instance, payload, &context)
            .await
            .with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let body = serde_json::to_vec(&payload).map_err(|error| ClassifiedReport::internal(&error).with_instance(uri.clone()))?;
    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            resource: ProvisionResource::TemporalEntities,
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: Some(&attr_id),
            method: Method::PATCH,
            body: Some(&body),
            context: &context,
            operation: ForwardingOperation::UpdateTemporalAttributeInstance,
            path_suffix: Some(&path_suffix),
            query_string: None,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
        })
        .await
        .with_instance(&uri)?;

    // Clause 5.6.14.4: an instance of an attribute a proxied registration
    // covers is modified at the source and needs no further processing.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        local_temporal_outcome(service.modify_attr_instance(&id, &attr_id, &instance, payload, &context).await, &forwarded)
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
