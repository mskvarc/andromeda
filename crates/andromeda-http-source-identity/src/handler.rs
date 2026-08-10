//! Retrieve Context Source Identity operation.

use crate::{
    error::{SourceIdentityError, classify},
    state::SourceIdentityState,
};
use andromeda_http_extraction::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};

/// Retrieve the configured context source identity.
pub async fn get(
    State(state): State<SourceIdentityState>,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
) -> Result<Response, ProblemResponse> {
    let value = serde_json::to_value(state.identity())
        .map_err(SourceIdentityError::from)
        .map_err(|error| classify(&error).with_instance(uri.clone()))?;
    let context = context.into_inner();
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .map_err(SourceIdentityError::from)
        .map_err(|error| classify(&error).with_instance(uri))?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
