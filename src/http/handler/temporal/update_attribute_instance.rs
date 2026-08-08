//! HTTP handling for temporal update attribute instance.

use crate::http::handler::temporal::{
    ContextUrl,
    IntoResponse,
    JsonLd,
    NgsiLdError,
    OriginalUri,
    Path,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    Value,
    parse_entity_id,
    validate_attr_name,
    validate_instance_id,
};

pub async fn update_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id, instance_id)): Path<(String, String, String)>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    if attr_id.is_empty() {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    validate_attr_name(&attr_id, &uri)?;
    validate_instance_id(&instance_id, &uri)?;
    let context = context.merge(body_context);
    service
        .modify_attr_instance(id.as_ref(), &attr_id, &instance_id, payload, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
