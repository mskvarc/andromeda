//! HTTP handling for the create entity operation.

use crate::entity_operations::{
    AcceptFormatNoGeo,
    BatchBody,
    BatchOperationResult,
    ContextUrl,
    EntityResultExt,
    EntityTenantStateOrCreate,
    EntityTypeValue,
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
    parse_batch_entities,
    public_error,
    start_rendering,
};
use andromeda_http_extraction::tenant_state;

pub(crate) async fn batch_create(
    tenant_state::ProvisionableTenantState(state): EntityTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    prefer: PreferHeader,
    IncomingVia(incoming_via): IncomingVia,
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
        .with_entity_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // Per spec 4.3.6.3: exclusive/redirect — don't create locally, return CS result
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(internal_error(&"Expected forwarded result", &uri));
        };
        let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::CREATED);
        if let Some(resp_body) = first.body() {
            return Ok((status, Json(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    // Inclusive or no match: create locally
    let mode = service.effective_side_effect_mode(async_preference(&prefer), entities.len());
    let batch = service.batch_create(&entities, &context, mode).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, super::super::entity_error::classify_entity_error);
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        let value = serde_json::to_value(&result.success).with_entity_instance(&uri)?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Batch(value))
            .render()
            .with_entity_instance(&uri)?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response(),
            mode,
        ))
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
