//! HTTP handling for the merge entity operation.

use crate::http::handler::entity_operations::{
    AcceptFormat,
    AndromedaError,
    BatchBody,
    BatchOperationResult,
    ContextUrl,
    EntityTypeValue,
    ForwardBatchProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    NgsiLdBatchResponse,
    NgsiLdError,
    OriginalUri,
    PreferHeader,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    SonicJson,
    StatusCode,
    TenantState,
    apply_preference_applied,
    async_preference,
    operation_result,
    parse_batch_entities,
    start_rendering,
};

pub async fn batch_merge(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
    BatchBody { headers, body }: BatchBody,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;

    let (entities, context, mut parse_errors) = parse_batch_entities(&body, &headers, &context, &uri)?;

    if entities.is_empty() && parse_errors.is_empty() {
        return Err(AndromedaError::entity_validation("Request body must contain a non-empty array of entities")
            .with_instance(&uri)
            .into());
    }

    // Distributed forwarding per spec 5.6.20
    let context_url = context.link_contexts().first();
    let entity_type = entities.first().and_then(|entity| match &entity.type_ {
        EntityTypeValue::Single(name) => Some(name),
        EntityTypeValue::Multiple(names) => names.first(),
    });
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::MergeBatch,
            batch_path: "/entityOperations/merge",
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(resp_body) = first.body() {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(async_preference(&prefer), entities.len());
    let batch = service.batch_merge(&entities, &context, mode).await.with_instance(&uri)?;
    let mut result: BatchOperationResult = operation_result(batch);
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        let value =
            serde_json::to_value(&result).map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response())
    }
}
