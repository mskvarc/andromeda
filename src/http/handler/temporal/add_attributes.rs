//! HTTP handling for temporal add attributes.

use crate::http::handler::temporal::{
    ContextUrl,
    IntoResponse,
    JsonLd,
    OriginalUri,
    ProblemResponse,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TypedPath,
    Uri,
    Value,
};

pub async fn add_attrs(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    service.add_attrs(&id, payload, &context).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
