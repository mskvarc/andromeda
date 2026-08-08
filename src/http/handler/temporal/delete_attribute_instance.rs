//! HTTP handling for temporal delete attribute instance.

use crate::http::handler::temporal::{
    ContextSpec,
    ContextUrl,
    EmptyParams,
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
    validate_instance_id,
};

pub async fn delete_attr_instance(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    Path((id, attr_id, instance_id)): Path<(String, String, String)>,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    validate_attr_name(&attr_id, &uri)?;
    validate_instance_id(&instance_id, &uri)?;
    let context: ContextSpec = context.into_inner();
    service
        .delete_attr_instance(id.as_ref(), &attr_id, &instance_id, &context)
        .await
        .with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
