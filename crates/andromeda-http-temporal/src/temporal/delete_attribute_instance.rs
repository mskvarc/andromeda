//! HTTP handling for temporal delete attribute instance.

use crate::temporal::{
    AttributeName,
    ContextSpec,
    ContextUrl,
    EmptyParams,
    InstanceId,
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

pub async fn delete_attr_instance(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id, instance_id)): TypedPath<(Uri, AttributeName, InstanceId)>,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    service.delete_attr_instance(&id, &attr_id, &instance_id, &context).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
