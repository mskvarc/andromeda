//! HTTP handling for temporal create or update (clause 5.6.11).

use crate::{
    forwarded_evolution_content::forwarded_content,
    request_context_admission::admit_request_context,
    temporal::{
        CreateOrUpdateResult,
        ForwardProvisionRequest,
        ForwardingOperation,
        HeaderMap,
        IncomingVia,
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
        TemporalError,
        TenantStateOrCreate,
        TriggeringRequest,
        ValidatedQuery,
        classify_temporal_error,
        collect_proxied_attrs,
        provision_response,
    },
    written_evolution_admission::admit_written_evolution,
};
use andromeda_http_extraction::{
    context_target,
    resource_location::{ResourceCollection, ResourceLocator},
};
use andromeda_ngsi_ld_model::temporal::written_temporal_entity::WrittenTemporalEntity;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_temporal::temporal_validation_reason::TemporalValidationReason;

/// The response a local create-or-update answers with: 201 with the new
/// evolution's location, or 204 (clause 5.6.11.5).
fn success_response(locator: &ResourceLocator, result: &Result<CreateOrUpdateResult, TemporalError>) -> Response {
    match result {
        Ok(CreateOrUpdateResult::Created(id)) => locator.locate(StatusCode::CREATED.into_response(), ResourceCollection::TemporalEntities, id),
        Ok(CreateOrUpdateResult::Updated) | Err(_) => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Create or Update Temporal Evolution of an Entity (clause 5.6.11): 201 with
/// the `Location` when the entity was created, 204 when it was updated, 207 with
/// the clause 5.2.16 result when only some part succeeded.
pub(crate) async fn create_or_update(
    // Clause 6.3.20: the parameters are read first, before the tenant is
    // resolved, so an undefined one is an `InvalidRequest` on any tenant.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ProvisionableTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    locator: ResourceLocator,
    JsonLd { body, context }: JsonLd<WrittenTemporalEntity>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_request_context(state.temporal.engine(), context, &uri).await?;
    let payload = body.admitted(&context).await?;
    let service = &state.temporal;
    // Clause 5.6.11.4 opens with the clause 5.5.4 validation and forwards only
    // afterwards.
    admit_written_evolution(&payload, &uri)?;

    if params.local.unwrap_or(false) {
        let result = service.create_or_update(&target, payload, &context).await;
        let success = success_response(&locator, &result);
        result.with_instance(&uri)?;
        return Ok(success);
    }

    // Clause 5.6.11.4 matches registrations by the evolution's id and type.
    let entity_id = payload
        .id
        .clone() // clone: the payload is still needed afterwards, so its id cannot be moved out of it
        .ok_or(TemporalError::Validation(TemporalValidationReason::EntityMissingId))
        .with_instance(&uri)?;
    let entity_type = payload
        .type_
        .as_ref()
        .and_then(|types| types.iter().next())
        .map(|name| TypeExprRaw::Type(name.clone()));
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::TemporalEntities,
        entity_id: &entity_id,
        entity_type_selector: entity_type.as_ref(),
        attribute_name: None,
        method: Method::POST,
        context: &context,
        operation: ForwardingOperation::UpsertTemporalEntity,
        path_suffix: None,
        query_string: None,
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = state.forwarding.plan_provision(&target, &request).await.with_instance(&uri)?;
    // The registrations the plan sends the evolution to read the document it
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

    // Clause 5.6.11.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains creates or updates the evolution here.
    let payload = service
        .without_claimed(payload, &collect_proxied_attrs(&forwarded), &context)
        .await
        .with_instance(&uri)?;
    let (local, success) = if forwarded.is_empty() || payload.has_attributes() {
        let result = service.create_or_update(&target, payload, &context).await;
        let success = success_response(&locator, &result);
        (LocalOutcome::processed(result, classify_temporal_error), success)
    } else {
        (LocalOutcome::NothingRemained, StatusCode::NO_CONTENT.into_response())
    };
    provision_response(&entity_id, &forwarded, &local, success, &uri)
}
