//! HTTP handling for temporal delete.

use crate::temporal::{IntoResponse, OriginalUri, ProblemResponse, Response, ResultInstanceExt, StatusCode, TenantState, TypedPath, Uri};
use andromeda_http_extraction::tenant_state;

pub async fn delete(
    tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(id): TypedPath<Uri>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    service.delete(&id).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
