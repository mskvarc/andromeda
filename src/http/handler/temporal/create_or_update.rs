//! HTTP handling for temporal create or update.

use crate::http::handler::temporal::{
    ContextUrl,
    CreateOrUpdateResult,
    EmptyParams,
    IntoResponse,
    JsonLd,
    NgsiLdError,
    OriginalUri,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantStateOrCreate,
    ValidatedQuery,
    Value,
};

pub async fn create_or_update(
    TenantStateOrCreate(state): TenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    ValidatedQuery(_params): ValidatedQuery<EmptyParams>,
    JsonLd {
        value: payload,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.temporal;
    let context = context.merge(body_context);
    let result = service.create_or_update(payload, &context).await.with_instance(&uri)?;

    match result {
        CreateOrUpdateResult::Created(id) => Ok((StatusCode::CREATED, [("Location", format!("/ngsi-ld/v1/temporal/entities/{}", id))]).into_response()),
        CreateOrUpdateResult::Updated => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}
