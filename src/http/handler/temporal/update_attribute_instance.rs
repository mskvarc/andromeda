//! HTTP handling for temporal update attribute instance.

use crate::http::handler::temporal::{
    AttributeName,
    ContextUrl,
    InstanceId,
    IntoResponse,
    JsonLd,
    NgsiLdError,
    OriginalUri,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TypedPath,
    Uri,
    Value,
};

pub async fn update_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id, instance_id)): TypedPath<(Uri, AttributeName, InstanceId)>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    service
        .modify_attr_instance(&id, &attr_id, &instance_id, payload, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
