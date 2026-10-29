//! HTTP handling for the batch delete entity operation (clause 5.6.10).

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchOperationResult,
    EntityResultExt,
    EntityTenantState,
    IntoResponse,
    NgsiLdRequestEnvelope,
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
    drop_absent_proxied,
    forward_identifiers,
    merge_forwarded,
    multi_status_response,
};
use andromeda_http_extraction::{
    body_preconditions::BodyPreconditions,
    context_target,
    entity_id_batch::{EntityIdBatch, JsonLdEntityIds},
};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_request::{local::LocalOnlyParams, query_narrowing::ExecutionScope};

pub(crate) async fn batch_delete(
    // The clause 6.3.4 preconditions are properties of the incoming request
    // alone, so they answer before the query is read or the tenant is resolved.
    _preconditions: BodyPreconditions,
    // Clause 6.3.18 is the one URL parameter the operation defines, so any
    // other is an `InvalidRequest`; it is read first, before the tenant is
    // resolved.
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    _accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    prefer: PreferHeader,
    // Clause 5.6.10.3 takes "A JSON-LD Array": the body passes the clause
    // 6.3.4 and 6.3.5 preconditions a JSON-LD document does, and clause
    // 5.6.10.4 reports an identifier refused on its own -- one too long to be
    // stored -- against itself while the rest are deleted.
    JsonLdEntityIds { batch, context }: JsonLdEntityIds,
) -> Result<Response, ProblemResponse> {
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let EntityIdBatch {
        ids: entity_ids,
        refused: mut id_errors,
    } = batch.admitted(&context).await?;
    let service = &state.entities;

    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(&headers),
    };
    let forwarded = forward_identifiers(&state, &target, &entity_ids, ExecutionScope::of_flag(params.local), context, triggering, &uri).await?;

    // Clause 5.6.10.4: every identifier is also deleted locally; an entity a
    // proxied registration holds is no local failure when it is absent here.
    let placement = service.side_effect_placement(&target, async_preference(prefer), entity_ids.len());
    let mut batch = service.batch_delete(placement, &entity_ids).await.with_entity_instance(&uri)?;
    drop_absent_proxied(&mut batch.errors, &forwarded);
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    result.errors.append(&mut id_errors);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), placement))
    } else {
        multi_status_response(&result, &uri, placement)
    }
}
