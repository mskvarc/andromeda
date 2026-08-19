//! HTTP handling for the delete entity operation.

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchOperationResult,
    Bytes,
    ContextUrl,
    EntityResultExt,
    EntityTenantState,
    ForwardBatchProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    NgsiLdBatchResponse,
    NgsiLdErrorType,
    OriginalUri,
    PreferHeader,
    ProblemResponse,
    Response,
    ResponseMediaType,
    ResponsePayload,
    StatusCode,
    apply_preference_applied,
    async_preference,
    batch_operation_result,
    internal_error,
    public_error,
    start_rendering,
};
use andromeda_http_extraction::tenant_state;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;

pub(crate) async fn batch_delete(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    _accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
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

    // Distributed forwarding per spec 5.6.10
    let context: ContextSpec = context.into_inner();
    let context_url = context.link_contexts().first();
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: None, // delete doesn't have entity type in payload
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::DeleteBatch,
            batch_path: "/entityOperations/delete",
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_entity_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        // Redirect: don't delete locally, return CS result
        let Some(first) = forwarded.first() else {
            return Err(internal_error(&"Expected forwarded result", &uri));
        };
        let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(resp_body) = first.body() {
            return Ok((status, Json(resp_body.clone())).into_response()); // clone: owned Value needed for JSON response
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(async_preference(&prefer), entity_ids.len());

    if modes.has_exclusive {
        // Exclusive: delete locally AND forwarded (already done)
        if let Err(e) = service.batch_delete(&entity_ids, mode).await {
            tracing::warn!(error = %e, "Failed to delete local entities during exclusive batch delete");
        }
        return Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode));
    }

    // Inclusive or no match: delete locally
    let batch = service.batch_delete(&entity_ids, mode).await.with_entity_instance(&uri)?;
    let result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, super::super::entity_error::classify_entity_error);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        // Clause 6.3.6: the 207 Multi-Status body is always application/json,
        // regardless of the negotiated Accept, so it carries no JSON-LD @context.
        let value = serde_json::to_value(&result).with_entity_instance(&uri)?;
        let rendered = start_rendering(ResponseMediaType::Json, &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Batch(value))
            .render()
            .with_entity_instance(&uri)?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}
