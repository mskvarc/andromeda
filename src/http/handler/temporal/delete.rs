//! HTTP handling for temporal delete.

use crate::http::handler::temporal::{IntoResponse, NgsiLdError, OriginalUri, Response, ResultInstanceExt, StatusCode, TenantState, TypedPath, Uri};

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, TypedPath(id): TypedPath<Uri>) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    service.delete(&id).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
