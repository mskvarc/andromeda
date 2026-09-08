//! HTTP handling for the batch merge entity operation (clause 5.6.20).

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchBody,
    BatchOperationResult,
    BatchProvisionOperation,
    ContextUrl,
    EntityResultExt,
    EntityTenantState,
    IncomingVia,
    IntoResponse,
    NgsiLdErrorType,
    OriginalUri,
    PreferHeader,
    ProblemResponse,
    Response,
    StatusCode,
    TriggeringRequest,
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

pub(crate) async fn batch_merge(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    _accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
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

    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(&headers),
    };
    let forwarded = forward_entities(&state, &target, BatchProvisionOperation::Merge, &entities, &context, None, triggering, &uri).await?;

    // Clause 5.6.20.4: the local phase merges what proxied registrations left.
    let remainder = local_remainder(entities, &forwarded);
    let mode = service.effective_side_effect_mode(async_preference(prefer), remainder.len());
    let batch = service.batch_merge(&target, &remainder, &context, mode).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut parse_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        multi_status_response(&result, &context, &uri, mode)
    }
}
