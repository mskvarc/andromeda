//! HTTP handling for temporal delete attribute.

use crate::http::handler::temporal::{
    AttributeName,
    ContextUrl,
    DeleteAttrParams,
    DeleteScope,
    IntoResponse,
    OriginalUri,
    ProblemResponse,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TypedPath,
    Uri,
    ValidatedQuery,
};

pub async fn delete_attr(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Uri, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.into_inner();
    let scope = if params.delete_all.unwrap_or(false) {
        DeleteScope::All
    } else {
        DeleteScope::Single
    };
    service
        .delete_attr(&id, &attr_id, params.dataset_id.as_ref(), scope, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
