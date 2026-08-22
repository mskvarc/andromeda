//! HTTP handling for the batch delete entity operation (clause 5.6.10).

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchOperationResult,
    Bytes,
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
    Uri,
    apply_preference_applied,
    async_preference,
    batch_operation_result,
    classify_entity_error,
    drop_absent_proxied,
    forward_identifiers,
    merge_forwarded,
    multi_status_response,
    public_error,
};
use andromeda_http_extraction::tenant_state;
use andromeda_jsonld::context_spec::ContextSpec;
use axum::http::HeaderMap;

pub(crate) async fn batch_delete(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    _accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;

    let entity_ids: Vec<Uri> = serde_json::from_slice(&body).map_err(|error| {
        public_error(
            NgsiLdErrorType::InvalidRequest,
            format!("Request body must be a JSON array of entity IDs: {error}"),
            &uri,
        )
    })?;

    if entity_ids.is_empty() {
        return Err(public_error(
            NgsiLdErrorType::BadRequestData,
            "Request body must contain a non-empty array of entity IDs",
            &uri,
        ));
    }

    let context: ContextSpec = context.into_inner();
    let triggering = TriggeringRequest {
        via: incoming_via.as_ref(),
        headers: Some(&headers),
    };
    let forwarded = forward_identifiers(&state, &entity_ids, &context, triggering, &uri).await?;

    // Clause 5.6.10.4: every identifier is also deleted locally; an entity a
    // proxied registration holds is no local failure when it is absent here.
    let mode = service.effective_side_effect_mode(async_preference(&prefer), entity_ids.len());
    let mut batch = service.batch_delete(&entity_ids, mode).await.with_entity_instance(&uri)?;
    drop_absent_proxied(&mut batch.errors, &forwarded);
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, classify_entity_error);
    merge_forwarded(&mut result, &forwarded);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        multi_status_response(&result, &context, &uri, mode)
    }
}
