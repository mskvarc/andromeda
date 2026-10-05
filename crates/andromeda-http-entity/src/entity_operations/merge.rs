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
        NgsiLdRequestEnvelope,
        ParsedBatch,
        PreferHeader,
        ProblemResponse,
        Response,
        StatusCode,
        TriggeringRequest,
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
    },
    written_payload_admission::admitted_batch_entities,
};
use andromeda_entity::write_kind::BatchMerge;
use andromeda_http_extraction::context_target;
use andromeda_ngsi_ld_request::empty::EmptyParams;

pub(crate) async fn batch_merge(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    _accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, .. }: NgsiLdRequestEnvelope,
    prefer: PreferHeader,
    batch_body: BatchBody,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    let ParsedBatch {
        items,
        context: _,
        errors: mut entity_errors,
    } = parse_batch_entities(&batch_body, &uri).await?;
    // Clause 5.6.20.4 runs clause 5.6.17 "for each of the NGSI-LD Entities
    // included in the input Array", each entity answering for itself: the
    // clause 5.5.4 validation an entity fails contributes a BatchEntityError of
    // its own rather than failing the whole request.
    let mut admitted = admitted_batch_entities(service, state.entity_query.engine(), BatchMerge, items, &mut entity_errors)
        .await
        .with_entity_instance(&uri)?;

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
        &admitted,
        &uri,
    )
    .await?;

    // Clause 5.6.20.4: the local phase merges what proxied registrations left.
    local_remainder(&mut admitted, &forwarded);
    let placement = service.side_effect_placement(&target, async_preference(prefer), admitted.len());
    let batch = service.batch_merge(placement, admitted).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut entity_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), placement))
    } else {
        multi_status_response(&result, &uri, placement)
    }
}
