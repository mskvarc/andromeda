//! HTTP handling for the delete by query entity operation.

use crate::http::handler::entity::{
    AndromedaError,
    BatchOperationResult,
    ContextSpec,
    ContextUrl,
    EntityQuery,
    ForwardBatchProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    NgsiLdError,
    OriginalUri,
    PurgeMode,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    operation_result,
};

pub async fn delete_by_query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    EntityQuery(query): EntityQuery,
) -> Result<Response, NgsiLdError> {
    let is_local = query.local.unwrap_or(false);
    let context: ContextSpec = context.into_inner();

    // Determine purge mode from drop/keep parameters (S5.6.21)
    let purge_mode = if let Some(ref drop_attrs) = query.drop {
        PurgeMode::Drop(drop_attrs.clone())
    } else if let Some(ref keep_attrs) = query.keep {
        PurgeMode::Keep(keep_attrs.clone())
    } else {
        PurgeMode::DeleteEntities
    };

    let compiled = state.entity_query.compile_query_uncapped(&query, &context).await.with_instance(&uri)?;
    let entity_ids = state.entity_query.query_ids(&compiled).await.with_instance(&uri)?;

    if entity_ids.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // When drop/keep is specified, modify attributes instead of deleting entities
    match purge_mode {
        PurgeMode::Drop(_) | PurgeMode::Keep(_) => {
            state.entities.purge_attrs(&entity_ids, &purge_mode, &context).await.with_instance(&uri)?;
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        PurgeMode::DeleteEntities => {}
    }

    if !is_local {
        let body =
            serde_json::to_vec(&entity_ids).map_err(|e| AndromedaError::internal(format!("Failed to serialize entity IDs: {}", e)).with_instance(&uri))?;
        let context_url = context.link_contexts().first();
        let forwarded = state
            .forwarding
            .forward_batch_provision(ForwardBatchProvisionRequest {
                entity_type_compact: None,
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
            let Some(first) = forwarded.first() else {
                return Err(NgsiLdError::from(AndromedaError::internal("Expected forwarded result").with_instance(&uri)));
            };
            let status = StatusCode::from_u16(first.status()).unwrap_or(StatusCode::NO_CONTENT);
            if let Some(resp_body) = first.body() {
                return Ok((status, Json(resp_body.clone())).into_response());
            }
            return Ok(status.into_response());
        }

        if modes.has_exclusive {
            if let Err(e) = state
                .entities
                .batch_delete(&entity_ids, andromeda_entity::side_effect::BatchSideEffectMode::Inline)
                .await
            {
                tracing::warn!(error = %e, "Failed to delete local entities during exclusive batch delete");
            }
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
    }

    let batch = state
        .entities
        .batch_delete(&entity_ids, andromeda_entity::side_effect::BatchSideEffectMode::Inline)
        .await
        .with_instance(&uri)?;
    let result: BatchOperationResult = operation_result(batch);

    if result.errors.is_empty() {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
    }
}
