//! HTTP handling for temporal add attributes.

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
};

pub async fn add_attrs(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path(id): Path<String>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    let context = context.merge(body_context);
    service.add_attrs(id.as_ref(), payload, &context).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
