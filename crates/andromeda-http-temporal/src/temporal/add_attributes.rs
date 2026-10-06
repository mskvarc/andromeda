//! HTTP handling for temporal add attributes (clause 5.6.12).

use crate::{
    forwarded_evolution_content::forwarded_content,
    request_context_admission::admit_request_context,
    temporal::{
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
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
        ValidatedQuery,
        collect_proxied_attrs,
        local_temporal_outcome,
        provision_response,
    },
    written_evolution_admission::admit_added_attributes,
};
use andromeda_http_extraction::context_target;
use andromeda_ngsi_ld_model::temporal::written_temporal_entity::WrittenTemporalEntity;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;

pub async fn add_attrs(
    // Clause 6.3.20: the parameters are read first, before the tenant is
    // resolved, so an undefined one is an `InvalidRequest` on any tenant.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Identifier>,
    JsonLd { body, context }: JsonLd<WrittenTemporalEntity>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_request_context(state.temporal.engine(), context, &uri).await?;
    let payload = body.admitted(&context)?;
    let service = &state.temporal;
    // Clause 5.6.12.4 runs the clause 5.5.4 validation before the forwarding
    // phase, the Fragment's `id` included (clause 5.4).
    admit_added_attributes(&id, &payload, &uri)?;

    if params.local.unwrap_or(false) {
        service.add_attrs(&target, &id, payload, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let entity_type = payload
        .type_
        .as_ref()
        .and_then(|types| types.iter().next())
        .map(|name| TypeExprRaw::Type(name.clone())); // clone: the one-type selector owns the type it names
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::TemporalEntities,
        entity_id: &id,
        entity_type_selector: entity_type.as_ref(),
        attribute_name: None,
        method: Method::POST,
        context: &context,
        operation: ForwardingOperation::AppendTemporalAttributes,
        path_suffix: Some("/attrs"),
        query_string: None,
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = state.forwarding.plan_provision(&target, &request).await.with_instance(&uri)?;
    // The registrations the plan sends the fragment to read the document it
    // expands to; a write no registration covers expands nothing.
    let document = if plan.reads_payload() {
        Some(service.forwardable_document(&payload, &context).await.with_instance(&uri)?)
    } else {
        None
    };
    let forwarded = state
        .forwarding
        .execute_provision(plan, &request, document.as_ref().map(forwarded_content))
        .await
        .with_instance(&uri)?;

    // Clause 5.6.12.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is added here.
    let payload = service
        .without_claimed(payload, &collect_proxied_attrs(&forwarded), &context)
        .await
        .with_instance(&uri)?;
    let local = if forwarded.is_empty() || payload.has_attributes() {
        local_temporal_outcome(service.add_attrs(&target, &id, payload, &context).await, &forwarded)
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
