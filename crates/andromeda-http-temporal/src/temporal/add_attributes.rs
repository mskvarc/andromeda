//! HTTP handling for temporal add attributes (clause 5.6.12).

use crate::{
    temporal::{
        ForwardProvisionRequest,
        ForwardingOperation,
        IntoResponse,
        JsonLd,
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
        Uri,
        ValidatedQuery,
        Value,
        collect_proxied_attrs,
        has_attributes,
        local_temporal_outcome,
        payload_entity_type,
        provision_response,
        strip_attributes_from_value,
    },
    written_evolution_admission::admit_written_evolution,
};
use andromeda_http_extraction::context_target;
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};

pub async fn add_attrs(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    // Clause 5.6.12.4 runs the clause 5.5.4 validation before the forwarding
    // phase.
    admit_written_evolution(service.engine(), &payload, &context, &uri).await?;

    if params.local.unwrap_or(false) {
        service.add_attrs(&target, &id, payload, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let entity_type = payload_entity_type(&payload);
    let body = serde_json::to_vec(&payload)
        .map_err(|error| ClassifiedReport::internal(&error, InternalFailureSummary::RESPONSE_ENCODING).with_instance(uri.clone()))?;
    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::TemporalEntities,
                entity_id: &id,
                entity_type_compact: entity_type.as_ref(),
                attribute_name: None,
                method: Method::POST,
                body: Some(&body),
                context: &context,
                operation: ForwardingOperation::AppendTemporalAttributes,
                path_suffix: Some("/attrs"),
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_instance(&uri)?;

    // Clause 5.6.12.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is added here.
    let mut payload = payload;
    strip_attributes_from_value(&mut payload, &collect_proxied_attrs(&forwarded));
    let local = if forwarded.is_empty() || has_attributes(&payload) {
        local_temporal_outcome(service.add_attrs(&target, &id, payload, &context).await, &forwarded)
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
