//! HTTP handling for temporal create or update.

use crate::temporal::{
    ContextUrl,
    CreateOrUpdateResult,
    EmptyParams,
    IntoResponse,
    JsonLd,
    OriginalUri,
    ProblemResponse,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantStateOrCreate,
    ValidatedQuery,
    Value,
};
use andromeda_http_extraction::tenant_state;
use andromeda_http_response::headers::with_location;

pub async fn create_or_update(
    tenant_state::ProvisionableTenantState(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    let result = service.create_or_update(payload, &context).await.with_instance(&uri)?;

    match result {
        CreateOrUpdateResult::Created(id) => Ok(with_location(
            StatusCode::CREATED.into_response(),
            format!("/ngsi-ld/v1/temporal/entities/{}", id),
        )),
        CreateOrUpdateResult::Updated => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}
