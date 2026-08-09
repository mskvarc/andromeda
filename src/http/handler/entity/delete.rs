//! HTTP handling for the delete entity operation.

use crate::http::handler::entity::{
    ContextSpec,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    LocalOnlyParams,
    NgsiLdError,
    OriginalUri,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TypedPath,
    Uri,
    ValidatedQuery,
    build_multi_status_body,
    has_remote_errors,
};

pub async fn delete(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let is_local = params.local.unwrap_or(false);

    if is_local {
        service.delete(&id).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context: ContextSpec = context.into_inner();
    let context_url = context.link_contexts().first();

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: None,
            method: reqwest::Method::DELETE,
            body: None,
            context_url,
            context: &context,
            operation: ForwardingOperation::DeleteEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        service.delete(&id).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive {
        if let Err(e) = service.delete(&id).await {
            tracing::warn!(error = %e, entity_id = %id, "Failed to delete local entity during exclusive forwarding");
        }
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    if modes.has_redirect {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    if modes.has_inclusive {
        let local_result = service.delete(&id).await;
        let local_ok = local_result.is_ok();
        let local_error = local_result.err();

        if local_ok && !has_remote_errors(&forwarded) {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }

        let body = build_multi_status_body(&id, local_ok, local_error.as_ref(), &forwarded);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    service.delete(&id).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
