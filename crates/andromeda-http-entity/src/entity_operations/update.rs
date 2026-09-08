//! HTTP handling for the batch update entity operation (clause 5.6.9).

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchBody,
    BatchOperationResult,
    BatchProvisionOperation,
    EntityResultExt,
    EntityTenantState,
    ForwardedBatch,
    IntoResponse,
    NgsiLdErrorType,
    NgsiLdRequestEnvelope,
    OverwritePolicy,
    PreferHeader,
    ProblemResponse,
    RequestOverwritePolicy,
    Response,
    StatusCode,
    TriggeringRequest,
    UpdateParams,
    ValidatedQuery,
    apply_preference_applied,
    async_preference,
    batch_operation_result,
    classify_entity_error,
    forward_entities,
    local_remainder,
    merge_forwarded,
    multi_status_response,
    parse_batch_entities,
    public_error,
};
use andromeda_http_extraction::context_target;

pub(crate) async fn batch_update(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope {
        uri, context, incoming_via, ..
    }: NgsiLdRequestEnvelope,
    _accept: AcceptFormatNoGeo,
    prefer: PreferHeader,
    ValidatedQuery(params): ValidatedQuery<UpdateParams>,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(public_error(
            NgsiLdErrorType::BadRequestData,
            "Request body must contain a non-empty array of entities",
            &uri,
        ));
    }

    let overwrite = match params.options {
        Some(RequestOverwritePolicy::NoOverwrite) => OverwritePolicy::Preserve,
        Some(RequestOverwritePolicy::Overwrite) | None => OverwritePolicy::Overwrite,
    };

    let query_string = params.options.map(|option| format!("options={option}"));
    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(&headers),
    };
    let forwarded = forward_entities(
        &state,
        &target,
        ForwardedBatch {
            operation: BatchProvisionOperation::Update(overwrite),
            query_string: query_string.as_deref(),
            triggering,
        },
        &entities,
        &context,
        &uri,
    )
    .await?;

    // Clause 5.6.9.4: the local phase updates what proxied registrations left.
    let remainder = local_remainder(entities, &forwarded);
    let mode = service.effective_side_effect_mode(async_preference(prefer), remainder.len());
    let batch = service
        .batch_update(&target, &remainder, &context, overwrite, mode)
        .await
        .with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut parse_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        multi_status_response(&result, &context, &uri, mode)
    }
}
