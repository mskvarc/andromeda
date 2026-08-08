//! HTTP handling for temporal delete attribute.

use crate::http::handler::temporal::{
    ContextUrl,
    DeleteAttrParams,
    DeleteScope,
    IntoResponse,
    NgsiLdError,
    OriginalUri,
    Path,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    ValidatedQuery,
    parse_entity_id,
    validate_attr_name,
};

pub async fn delete_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id)): Path<(String, String)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    let context = context.into_inner();
    let scope = if params.delete_all.unwrap_or(false) {
        DeleteScope::All
    } else {
        DeleteScope::Single
    };
    service
        .delete_attr(id.as_ref(), &attr_id, params.dataset_id.as_ref(), scope, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
