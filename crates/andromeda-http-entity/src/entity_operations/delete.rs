//! HTTP handling for the batch delete entity operation (clause 5.6.10).

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchOperationResult,
    ContextUrl,
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
    context_target,
    entity_id_batch::{EntityIdBatch, parse_batch_entity_ids},
    request_payload::RequestPayload,
};
use andromeda_ngsi_ld_request::empty::EmptyParams;

pub(crate) async fn batch_delete(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    _accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ContextUrl(context): ContextUrl,
    prefer: PreferHeader,
    // Clause 6.3.2: "'Length required' (411) ... shall be raised when an HTTP
    // request provided by a client does not define the 'Content-Length' HTTP
    // header." This operation reads a raw body rather than going through the
    // `JsonLd` extractor, and `RequestPayload` applies the precondition before
    // it reads the body within the broker's size limit.
    RequestPayload(body): RequestPayload,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    // Clause 5.6.10.4: an identifier refused on its own -- one too long to be
    // stored -- is reported against itself, and the rest are deleted.
    let EntityIdBatch {
        ids: entity_ids,
        refused: mut id_errors,
    } = parse_batch_entity_ids(&body, &uri)?;

    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(&headers),
    };
    let forwarded = forward_identifiers(&state, &target, &entity_ids, context, triggering, &uri).await?;

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
