//! HTTP handling for the append attributes entity operation.

use crate::http::handler::entity::{
    AndromedaError,
    AppendParams,
    AttributeName,
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
    OverwritePolicy,
    Path,
    RequestOverwritePolicy,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    ValidatedQuery,
    Value,
    collect_forwarded_exclusive_attrs,
    entity_error,
    first_forwarded_status,
    has_remote_errors,
    parse_entity_id,
    strip_exclusive_attrs_from_value,
};

pub async fn append_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(params): ValidatedQuery<AppendParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    let no_overwrite = params.options == Some(RequestOverwritePolicy::NoOverwrite);
    let overwrite = if no_overwrite {
        OverwritePolicy::Preserve
    } else {
        OverwritePolicy::Overwrite
    };

    // Forward to matching context sources
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let fragment_body =
        serde_json::to_vec(&fragment).map_err(|e| AndromedaError::internal(format!("Failed to serialize fragment: {}", e)).with_instance(&uri))?;
    let entity_id = id.to_string();
    let qs = if no_overwrite { "options=noOverwrite" } else { "" };

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: None,
            attribute_name: None,
            method: reqwest::Method::POST,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::AppendEntityAttributes,
            path_suffix: Some("/attrs"),
            query_string: if qs.is_empty() { None } else { Some(qs) },
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — append locally
        let (all_attrs, not_appended) = service.append_attrs(&id, fragment, overwrite, &context).await.with_instance(&uri)?;

        if not_appended.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let updated: Vec<&AttributeName> = all_attrs.iter().filter(|a| !not_appended.contains(&a.to_string())).collect();
        let not_updated_details: Vec<serde_json::Value> = not_appended
            .iter()
            .map(|attr| serde_json::json!({ "attributeName": attr, "reason": "Already exists" }))
            .collect();
        let body = serde_json::json!({ "updated": updated, "notUpdated": not_updated_details });
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let mut local_fragment = fragment;
    if !exclusive_attrs.is_empty() {
        strip_exclusive_attrs_from_value(&mut local_fragment, &exclusive_attrs);
    }

    // Apply locally (ignore errors for inclusive -- remote already succeeded)
    let local_result = service.append_attrs(&id, local_fragment, overwrite, &context).await;
    let remote_errors = has_remote_errors(&forwarded);

    if let Ok((_, not_appended)) = &local_result
        && !remote_errors
        && not_appended.is_empty()
    {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // 207 Multi-Status for mixed results
    let mut errors = Vec::new();
    if let Err(err) = local_result {
        errors.push(serde_json::to_value(entity_error(&entity_id, &err)).unwrap_or_default());
    }
    for r in &forwarded {
        if r.is_failure() {
            let error_body = r
                .body()
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "status": r.status(), "title": "Error from context source" }));
            errors.push(serde_json::json!({ "entityId": entity_id, "error": error_body }));
        }
    }
    let body = serde_json::json!({ "success": [entity_id], "errors": errors });
    Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
}
