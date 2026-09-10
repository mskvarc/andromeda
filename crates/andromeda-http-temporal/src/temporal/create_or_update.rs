//! HTTP handling for temporal create or update (clause 5.6.11).

use crate::{
    temporal::{
        ContextUrl,
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
        Uri,
        ValidatedQuery,
        Value,
        classify_temporal_error,
        collect_proxied_attrs,
        has_attributes,
        payload_entity_type,
        provision_response,
        strip_attributes_from_value,
    },
    written_evolution_admission::admit_written_evolution,
};
use andromeda_http_extraction::context_target;
use andromeda_http_response::{classified_report::ClassifiedReport, headers::with_location};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// The response a local create-or-update answers with: 201 with the new
/// evolution's location, or 204 (clause 5.6.11.5).
fn success_response(result: &Result<CreateOrUpdateResult, TemporalError>) -> Response {
    match result {
        Ok(CreateOrUpdateResult::Created(id)) => with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/temporal/entities/{id}")),
        Ok(CreateOrUpdateResult::Updated) | Err(_) => StatusCode::NO_CONTENT.into_response(),
    }
}

pub async fn create_or_update(
    context_target::ProvisionableTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
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
    // Clause 5.6.11.4 opens with the clause 5.5.4 validation and forwards only
    // afterwards.
    admit_written_evolution(&payload, &uri)?;

    if params.local.unwrap_or(false) {
        let result = service.create_or_update(&target, payload, &context).await;
        let success = success_response(&result);
        result.with_instance(&uri)?;
        return Ok(success);
    }

    // Clause 5.6.11.4 matches registrations by the evolution's id and type.
    let entity_id = payload
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uri>().ok())
        .ok_or_else(|| ClassifiedReport::public(NgsiLdErrorType::BadRequestData, "Entity id must be a valid URI").with_instance(uri.clone()))?;
    let entity_type = payload_entity_type(&payload);
    let body = serde_json::to_vec(&payload).map_err(|error| ClassifiedReport::internal(&error).with_instance(uri.clone()))?;
    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::TemporalEntities,
                entity_id: &entity_id,
                entity_type_compact: entity_type.as_ref(),
                attribute_name: None,
                method: Method::POST,
                body: Some(&body),
                context: &context,
                operation: ForwardingOperation::UpsertTemporalEntity,
                path_suffix: None,
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_instance(&uri)?;

    // Clause 5.6.11.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains creates or updates the evolution here.
    let mut payload = payload;
    strip_attributes_from_value(&mut payload, &collect_proxied_attrs(&forwarded));
    let (local, success) = if forwarded.is_empty() || has_attributes(&payload) {
        let result = service.create_or_update(&target, payload, &context).await;
        let success = success_response(&result);
        (LocalOutcome::processed(result, classify_temporal_error), success)
    } else {
        (LocalOutcome::NothingRemained, StatusCode::NO_CONTENT.into_response())
    };
    provision_response(&entity_id, &forwarded, &local, success, &uri)
}
