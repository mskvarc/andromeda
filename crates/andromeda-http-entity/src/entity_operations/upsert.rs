//! HTTP handling for the batch upsert entity operation (clause 5.6.8).

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
};
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

    let upsert_mode = params.options.unwrap_or_default();
    let upsert_policy = match upsert_mode {
        UpsertMode::Update => UpsertPolicy::Update,
        UpsertMode::Replace => UpsertPolicy::Replace,
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
            operation: BatchProvisionOperation::Upsert(upsert_mode),
            query_string: query_string.as_deref(),
            triggering,
        },
        &entities,
        &context,
        &uri,
    )
    .await?;

    // Clause 5.6.8.4: the local phase creates or updates what proxied
    // registrations left.
    let remainder = local_remainder(entities, &forwarded);
    let mode = service.effective_side_effect_mode(async_preference(prefer), remainder.len());
    let (batch, mut created) = service
        .batch_upsert(&target, &remainder, &context, upsert_policy, mode)
        .await
        .with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut parse_errors);
    merge_forwarded(&mut result, &forwarded);
    created.extend(created_remotely(&forwarded).cloned()); // clone: the response body owns the identifiers it lists

    // Clause 5.6.8.5: nothing when every entity already existed, the created
    // identifiers when some were created, the full result on any failure.
    if !result.errors.is_empty() {
        multi_status_response(&result, &context, &uri, mode)
    } else if created.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        created_response(&created, accept, &context, &uri, mode)
    }
}
