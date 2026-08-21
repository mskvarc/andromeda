//! Retrieve Context Source Identity operation (clauses 5.15.1 and 6.33.3.1).
//!
//! Clause 5.15.1.4 lets an implementation that does not support the operation
//! answer `NotImplemented`; Andromeda always knows its identity, so that branch
//! is unreachable by construction and the handler has no failure of its own
//! beyond rendering.

use crate::{
    error::{SourceIdentityError, classify},
    state::SourceIdentityState,
};
use andromeda_http_extraction::{accept::AcceptFormatNoGeo, context::ContextUrl, prefer::PreferHeader};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};
use chrono::Utc;

/// Retrieve the configured context source identity.
pub async fn get(
    State(state): State<SourceIdentityState>,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
) -> Result<Response, ProblemResponse> {
    let value = serde_json::to_value(state.identity_at(Utc::now()))
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
