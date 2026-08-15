//! HTTP handling for the update entity operation.

use crate::entity_operations::{
    AcceptFormat,
    BatchBody,
    BatchOperationResult,
    ContextUrl,
    EntityResultExt,
    EntityTenantState,
    EntityTypeValue,
    ForwardBatchProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    NgsiLdBatchResponse,
    NgsiLdErrorType,
    OriginalUri,
    OverwritePolicy,
    PreferHeader,
    ProblemResponse,
    RequestOverwritePolicy,
    Response,
    ResponsePayload,
    SonicJson,
    StatusCode,
    UpdateParams,
    ValidatedQuery,
    apply_preference_applied,
    async_preference,
    batch_operation_result,
    internal_error,
    parse_batch_entities,
    public_error,
    start_rendering,
};
use andromeda_http_extraction::tenant_state;

pub(crate) async fn batch_update(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    prefer: PreferHeader,
    ValidatedQuery(params): ValidatedQuery<UpdateParams>,
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

    let no_overwrite = params.options == Some(RequestOverwritePolicy::NoOverwrite);
    let overwrite = if no_overwrite {
        OverwritePolicy::Preserve
    } else {
        OverwritePolicy::Overwrite
    };

    // Distributed forwarding per spec 5.6.9
    let context_url = context.link_contexts().first();
    let entity_type = entities.first().and_then(|entity| match &entity.type_ {
        EntityTypeValue::Single(name) => Some(name),
        EntityTypeValue::Multiple(names) => names.first(),
    });
    let qs = params.options.map(|option| format!("options={option}"));
    let forwarded = state
        .forwarding
        .forward_batch_provision(ForwardBatchProvisionRequest {
            entity_type_compact: entity_type,
            body: &body,
            context_url,
            context: &context,
            operation: ForwardingOperation::UpdateBatch,
            batch_path: "/entityOperations/update",
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_entity_instance(&uri)?;

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        // SAFETY: guarded by has_exclusive/has_redirect which require non-empty forwarded
        let Some(first) = forwarded.first() else {
            return Err(internal_error(&"Expected forwarded result", &uri));
        };
        let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::NO_CONTENT);
        if let Some(resp_body) = first.body() {
            return Ok((status, SonicJson(resp_body.clone())).into_response());
        }
        return Ok(status.into_response());
    }

    let mode = service.effective_side_effect_mode(async_preference(&prefer), entities.len());
    let batch = service.batch_update(&entities, &context, overwrite, mode).await.with_entity_instance(&uri)?;
    let mut result: BatchOperationResult = batch_operation_result(batch.success, batch.errors, super::super::entity_error::classify_entity_error);
    result.errors.append(&mut parse_errors);

    if result.errors.is_empty() {
        Ok(apply_preference_applied(StatusCode::NO_CONTENT.into_response(), mode))
    } else {
        let value = serde_json::to_value(&result).with_entity_instance(&uri)?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Batch(value))
            .render()
            .with_entity_instance(&uri)?;
        Ok(apply_preference_applied(
            NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
            mode,
        ))
    }
}
