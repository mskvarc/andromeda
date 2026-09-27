//! HTTP handling for the batch upsert entity operation (clause 5.6.8).

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
        UpsertMode,
        UpsertParams,
        UpsertPolicy,
        ValidatedQuery,
        apply_preference_applied,
        async_preference,
        batch_operation_result,
        classify_entity_error,
        created_remotely,
        created_response,
        forward_entities,
        local_remainder,
        merge_forwarded,
        multi_status_response,
        parse_batch_entities,
        public_error,
    },
    written_payload_admission::admitted_batch_entities,
};
use andromeda_entity::write_kind::BatchUpsert;
use andromeda_http_extraction::context_target;

pub(crate) async fn batch_upsert(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri, context, incoming_via, ..
    }: NgsiLdRequestEnvelope,
    prefer: PreferHeader,
    ValidatedQuery(params): ValidatedQuery<UpsertParams>,
    batch_body: BatchBody,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    let (entities, context, mut entity_errors) = parse_batch_entities(&batch_body, &context, &uri)?;
    // Clause 5.6.8.4 runs clause 5.6.1, or clause 5.6.2 and a replacement where
    // the Entity already exists, "for each of the NGSI-LD Entities included in
    // the input Array": every entity answers for itself, so the clause 5.5.4
    // validation an entity fails contributes a BatchEntityError of its own
    // rather than failing the whole request.
    let upsert_mode = params.mode().with_entity_instance(&uri)?;
    let upsert_policy = match upsert_mode {
        UpsertMode::Update => UpsertPolicy::Update,
        UpsertMode::Replace => UpsertPolicy::Replace,
    };
    // Which NGSI-LD Nulls the payload may carry is the requested mode's answer
    // (clause 5.6.8.4), so the mode is read before the screening.
    let mut admitted = admitted_batch_entities(service, BatchUpsert(upsert_policy), entities, &context, &mut entity_errors)
        .await
        .with_entity_instance(&uri)?;

    if admitted.is_empty() && entity_errors.is_empty() {
        return Err(public_error(
            NgsiLdErrorType::BadRequestData,
            "request body must contain a non-empty array of entities",
            &uri,
        ));
    }

    let query_string = Some(format!("options={upsert_mode}"));
    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(batch_body.headers()),
    };
    let forwarded = forward_entities(
        &state,
        &target,
        ForwardedBatch {
            operation: BatchProvisionOperation::Upsert(upsert_mode),
            query_string: query_string.as_deref(),
            triggering,
        },
        &admitted,
        &context,
        &uri,
    )
    .await?;

    // Clause 5.6.8.4: the local phase creates or updates what proxied
    // registrations left.
    local_remainder(&mut admitted, &forwarded);
    let placement = service.side_effect_placement(&target, async_preference(prefer), admitted.len());
    let (batch, mut created) = service.batch_upsert(placement, admitted, &context).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut entity_errors);
    merge_forwarded(&mut result, &forwarded);
    created.extend(created_remotely(&forwarded).cloned()); // clone: the response body owns the identifiers it lists

    // Clause 5.6.8.5: nothing when every entity already existed, the created
    // identifiers when some were created, the full result on any failure.
    if !result.errors.is_empty() {
        multi_status_response(&result, &context, &uri, placement)
    } else if created.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), placement))
    } else {
        created_response(&created, accept, &context, &uri, placement)
    }
}
