//! HTTP handling for the update attributes entity operation.

use crate::http::handler::entity::{
    AndromedaError,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    JsonLd,
    NgsiLdError,
    OriginalUri,
    Path,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    Value,
    build_multi_status_body,
    collect_forwarded_exclusive_attrs,
    first_forwarded_status,
    has_remote_errors,
    parse_entity_id,
    strip_exclusive_attrs_from_value,
};

pub async fn update_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);

    // Forward to matching context sources
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let fragment_body =
        serde_json::to_vec(&fragment).map_err(|e| AndromedaError::internal(format!("Failed to serialize fragment: {}", e)).with_instance(&uri))?;
    let entity_id = id.to_string();

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            attribute_name: None,
            method: reqwest::Method::PATCH,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::UpdateEntityAttributes,
            path_suffix: Some("/attrs"),
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        let not_updated = service.update_attrs(&id, fragment, &context).await.with_instance(&uri)?;
        if not_updated.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let not_updated_details: Vec<serde_json::Value> = not_updated
            .iter()
            .map(|attr| serde_json::json!({ "attributeName": attr, "reason": "Attribute not found" }))
            .collect();
        let body = serde_json::json!({ "updated": [], "notUpdated": not_updated_details });
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Strip exclusive attributes before local update
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let mut local_fragment = fragment;
    if !exclusive_attrs.is_empty() {
        strip_exclusive_attrs_from_value(&mut local_fragment, &exclusive_attrs);
    }

    // Exclusive-only updates may strip all local attributes; in that case
    // local processing is complete and only remote status matters.
    if local_fragment.as_object().is_some_and(|obj| obj.is_empty()) {
        if !has_remote_errors(&forwarded) {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let body = build_multi_status_body(&entity_id, true, None, &forwarded);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let local_result = service.update_attrs(&id, local_fragment, &context).await;

    if local_result.is_ok() && !has_remote_errors(&forwarded) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let body = build_multi_status_body(&entity_id, local_result.is_ok(), local_result.err().as_ref(), &forwarded);
    Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
}
