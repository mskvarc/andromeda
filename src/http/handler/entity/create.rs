//! HTTP handling for the create entity operation.

use crate::http::handler::entity::{
    AndromedaError,
    ContextUrl,
    Entity,
    EntityTypeName,
    EntityTypeValue,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    JsonLd,
    LocalOnlyParams,
    NgsiLdError,
    OriginalUri,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantStateOrCreate,
    ValidatedQuery,
    build_multi_status_body,
    first_forwarded_status,
    has_remote_errors,
};

pub async fn create(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: entity,
        context: body_context,
    }: JsonLd<Entity>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);

    if is_local {
        let id = service.create(&entity, &context).await.with_instance(&uri)?;
        return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response());
    }

    // Check for matching CSRs for distributed forwarding
    let context_url = context.link_contexts().first().map(|s| s.as_str());
    let entity_body = serde_json::to_vec(&entity).map_err(|e| AndromedaError::internal(format!("Failed to serialize entity: {e}")).with_instance(&uri))?;
    let entity_id = entity.id.to_string();
    let entity_type_str: &str = match &entity.type_ {
        EntityTypeValue::Single(s) => s.as_str(),
        EntityTypeValue::Multiple(v) => v.first().map_or("", EntityTypeName::as_str),
    };

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: Some(&entity_id),
            entity_type_compact: Some(entity_type_str),
            attribute_name: None,
            method: reqwest::Method::POST,
            body: Some(&entity_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::CreateEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        let id = service.create(&entity, &context).await.with_instance(&uri)?;
        return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_exclusive || modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 201)).unwrap_or(StatusCode::CREATED);
        if status == StatusCode::CREATED {
            return Ok((status, [("Location", format!("/ngsi-ld/v1/entities/{}", entity_id))]).into_response());
        }
        return Ok(status.into_response());
    }

    if modes.has_inclusive {
        let local_result = service.create(&entity, &context).await;
        let local_ok = local_result.is_ok();
        let local_error = local_result.err();

        if local_ok && !has_remote_errors(&forwarded) {
            return Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", entity_id))]).into_response());
        }

        let body = build_multi_status_body(&entity_id, local_ok, local_error.as_ref(), &forwarded);
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    let id = service.create(&entity, &context).await.with_instance(&uri)?;
    Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/entities/{}", id))]).into_response())
}
