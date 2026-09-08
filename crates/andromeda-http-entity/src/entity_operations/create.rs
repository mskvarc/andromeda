//! HTTP handling for the batch create entity operation (clause 5.6.7).

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchBody,
    BatchOperationResult,
    BatchProvisionOperation,
    ContextUrl,
    EntityResultExt,
    EntityTenantStateOrCreate,
    IncomingVia,
    NgsiLdErrorType,
    OriginalUri,
    PreferHeader,
    ProblemResponse,
    Response,
    TriggeringRequest,
    async_preference,
    batch_operation_result,
    classify_entity_error,
    created_response,
    forward_entities,
    local_remainder,
    merge_forwarded,
    multi_status_response,
    parse_batch_entities,
    public_error,
};
use andromeda_http_extraction::context_target;

pub(crate) async fn batch_create(
    context_target::ProvisionableTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
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
    let forwarded = forward_entities(&state, &target, BatchProvisionOperation::Create, &entities, &context, None, triggering, &uri).await?;

    // Clause 5.6.7.4: the local phase creates what proxied registrations left.
    let remainder = local_remainder(entities, &forwarded);
    let mode = service.effective_side_effect_mode(async_preference(prefer), remainder.len());
    let batch = service.batch_create(&target, &remainder, &context, mode).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut parse_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        created_response(&result.success, accept, &context, &uri, mode)
    } else {
        multi_status_response(&result, &context, &uri, mode)
    }
}
