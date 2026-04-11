//! Handler for the `/info` endpoint.

use crate::{
    api::state::AppState,
    core::error::AndromedaError,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader},
        model::info::ContextSourceIdentity,
        response::single::NgsiLdResponse,
    },
    renderer::response::ResponseRenderer,
};
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/info/sourceIdentity -- Retrieve context source identity.
pub async fn get(State(state): State<AppState>, OriginalUri(uri): OriginalUri, accept: AcceptFormat, context: ContextUrl) -> Result<Response, NgsiLdError> {
    let identity = ContextSourceIdentity {
        id: state.source_identity,
        type_: "ContextSourceIdentity",
    };
    let context = context.into_inner();
    let value = serde_json::to_value(&identity)
        .map_err(|e| AndromedaError::internal(format!("Failed to serialize context source identity: {}", e)).with_instance(uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
