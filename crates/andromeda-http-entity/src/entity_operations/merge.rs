//! HTTP handling for the batch merge entity operation (clause 5.6.20).

use crate::{
    entity_operations::{
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
    },
    written_payload_admission::admitted_batch_entities,
};
use andromeda_entity::write_admission::BatchWrite;
use andromeda_http_extraction::context_target;

pub(crate) async fn batch_merge(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    _accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri, context, incoming_via, ..
    }: NgsiLdRequestEnvelope,
    prefer: PreferHeader,
    batch_body: BatchBody,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    let (entities, context, mut entity_errors) = parse_batch_entities(&batch_body, &context, &uri)?;
    // Clause 5.6.20.4 runs clause 5.6.17 "for each of the NGSI-LD Entities
    // included in the input Array", each entity answering for itself: the
    // clause 5.5.4 validation an entity fails contributes a BatchEntityError of
    // its own rather than failing the whole request.
    let entities = admitted_batch_entities(service, BatchWrite::Merge, entities, &context, &mut entity_errors)
        .await
        .with_entity_instance(&uri)?;

    if entities.is_empty() && entity_errors.is_empty() {
        return Err(public_error(
            NgsiLdErrorType::BadRequestData,
            "Request body must contain a non-empty array of entities",
            &uri,
        ));
    }

    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(batch_body.headers()),
    };
    let forwarded = forward_entities(
        &state,
        &target,
        ForwardedBatch {
            operation: BatchProvisionOperation::Merge,
            query_string: None,
            triggering,
        },
        &entities,
        &context,
        &uri,
    )
    .await?;

    // Clause 5.6.20.4: the local phase merges what proxied registrations left.
    let remainder = local_remainder(entities, &forwarded);
    let mode = service.effective_side_effect_mode(async_preference(prefer), remainder.len());
    let batch = service.batch_merge(&target, &remainder, &context, mode).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut entity_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        multi_status_response(&result, &context, &uri, mode)
    }
}
