//! HTTP handling for temporal delete.

use crate::http::handler::temporal::{IntoResponse, NgsiLdError, OriginalUri, Path, Response, ResultInstanceExt, StatusCode, TenantState, parse_entity_id};

pub async fn delete(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(id): Path<String>) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let id = parse_entity_id(&id, &uri)?;
    service.delete(id.as_ref()).await.with_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
