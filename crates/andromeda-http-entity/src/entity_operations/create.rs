//! HTTP handling for the batch create entity operation (clause 5.6.7).

use crate::{
    batch_payload_admission::{AdmittedBatchRequest, admitted_batch_entities},
    entity_operations::{
        AcceptFormatNoGeo,
        ApiRoot,
        BatchBody,
        BatchOperationResult,
        BatchProvisionOperation,
        EntityResultExt,
        EntityTenantStateOrCreate,
        ForwardedBatch,
        NgsiLdRequestEnvelope,
        PreferHeader,
        ProblemResponse,
        Response,
        TriggeringRequest,
        ValidatedQuery,
        async_preference,
        batch_operation_result,
        classify_entity_error,
        created_response,
        forward_entities,
        local_remainder,
        merge_forwarded,
        multi_status_response,
    },
};
use andromeda_entity::write_kind::BatchCreate;
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::context_target;
use andromeda_ngsi_ld_request::{local::LocalOnlyParams, query_narrowing::ExecutionScope};

/// Batch Entity Creation (clause 5.6.7): 201 listing the created identifiers when
/// every entity was created, 207 with the clause 5.2.16 result otherwise.
pub(crate) async fn batch_create(
    // Clause 6.3.18 is the one URL parameter the operation defines, so any
    // other is an `InvalidRequest`; it is read first, before the tenant is
    // resolved.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ProvisionableTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantStateOrCreate,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, .. }: NgsiLdRequestEnvelope,
    api_root: ApiRoot,
    prefer: PreferHeader,
    batch_body: BatchBody,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    // Clause 5.6.7.4 runs clause 5.6.1 "for each of the NGSI-LD Entities
    // included in the input Array", and "if the Entity creation failed, then a
    // new BatchEntityError shall be added to E containing the failed Entity ID
    // and the related ProblemDetails": the clause 5.5.4 validation an entity
    // fails is that entity's failure, not the request's.
    let AdmittedBatchRequest {
        mut admitted,
        context,
        refused: mut entity_errors,
    } = admitted_batch_entities(service, state.entity_query.engine(), BatchCreate, &batch_body, &uri).await?;

    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(batch_body.headers()),
    };
    let forwarded = forward_entities(
        &state,
        &target,
        ForwardedBatch {
            scope: ExecutionScope::of_flag(params.local),
            operation: BatchProvisionOperation::Create,
            query_string: None,
            triggering,
        },
        &admitted,
        &uri,
    )
    .await?;

    // Clause 5.6.7.4: the local phase creates what proxied registrations left.
    local_remainder(&mut admitted, &forwarded);
    let placement = service.side_effect_placement(&target, async_preference(prefer), admitted.len());
    let batch = service.batch_create(placement, admitted).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut entity_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        created_response(
            &result.success,
            accept,
            ResponseContextSource::new(state.entity_query.engine(), &context, &state.context_hosting, &api_root),
            &uri,
            placement,
        )
        .await
    } else {
        multi_status_response(&result, &uri, placement)
    }
}
