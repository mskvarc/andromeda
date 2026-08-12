//! HTTP handling for temporal delete attribute instance.

use crate::temporal::{
    AttributeName,
    ContextSpec,
    ContextUrl,
    EmptyParams,
    InstanceFamily,
    InstanceIdentity,
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
    TypedPath((id, attr_id, instance_id)): TypedPath<(Uri, AttributeName, Uri)>,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    // Clause 4.5.7 carries an `instanceId` as a URI; which of the two stored
    // shapes it names is decided here, at the edge, so the store addresses one
    // identity column rather than searching both.
    let instance = InstanceIdentity::from_uri(instance_id, InstanceFamily::Attribute);
    service.delete_attr_instance(&id, &attr_id, &instance, &context).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
