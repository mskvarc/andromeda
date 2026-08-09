//! HTTP handling for the create entity operation.

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
    RespondMode,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    SonicJson,
    StatusCode,
    TenantStateOrCreate,
    apply_preference_applied,
    operation_result,
    parse_batch_entities,
    start_rendering,
};

pub async fn batch_create(
    TenantStateOrCreate(state): TenantStateOrCreate,
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

    // Distributed forwarding: forward to matching context sources
    // Per spec 5.6.7: iterate CSRs, forward matching entities
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
            operation: ForwardingOperation::CreateBatch,
            batch_path: "/entityOperations/create",
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // Per spec 4.3.6.3: exclusive/redirect — don't create locally, return CS result
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
        };
        let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::CREATED);
        if let Some(resp_body) = first.body() {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: create locally
    let mode = service.effective_side_effect_mode(prefer.respond_mode == Some(RespondMode::Async), entities.len());
    let batch = service.batch_create(&entities, &context, mode).await.with_instance(&uri)?;
    let mut result: BatchOperationResult = operation_result(batch);
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        let value = serde_json::to_value(&result.success)
            .map_err(|e| AndromedaError::internal(format!("Failed to serialize batch result: {}", e)).with_instance(&uri))?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Batch(value))
            .render()?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response(),
            mode,
        ))
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
