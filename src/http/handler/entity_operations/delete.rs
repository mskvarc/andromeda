//! HTTP handling for the delete entity operation.

use crate::http::handler::entity_operations::{
    AcceptFormat,
    AndromedaError,
    BatchOperationResult,
    Bytes,
    ContextUrl,
    ForwardBatchProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    NgsiLdBatchResponse,
    NgsiLdError,
    OriginalUri,
    PreferHeader,
    RespondMode,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    SonicJson,
    StatusCode,
    TenantState,
    apply_preference_applied,
    operation_result,
    start_rendering,
};

pub async fn batch_delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    body: Bytes,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let entity_ids: Vec<String> = serde_json::from_slice(&body)
        .map_err(|e| AndromedaError::malformed_request(format!("Request body must be a JSON array of entity IDs: {}", e)).with_instance(&uri))?;

    if entity_ids.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entity IDs")
            .with_instance(&uri)
            .into());
    }

    // Distributed forwarding per spec 5.6.10
    let context: andromeda_jsonld::context_spec::ContextSpec = context.into_inner();
    let context_url = context.link_contexts().first().map(|s| s.as_str());
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
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        // Redirect: don't delete locally, return CS result
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(resp_body) = first.body() {
            return Ok((status, SonicJson(resp_body.clone())).into_response()); // clone: owned Value needed for JSON response
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entity_ids.len());

    if modes.has_exclusive {
        // Exclusive: delete locally AND forwarded (already done)
        if let Err(e) = service.batch_delete(&entity_ids, mode).await {
            tracing::warn!(error = %e, "Failed to delete local entities during exclusive batch delete");
        }
        return Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode));
    }

    // Inclusive or no match: delete locally
    let batch = service.batch_delete(&entity_ids, mode).await.with_instance(&uri)?;
    let result: BatchOperationResult = operation_result(batch);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}
