//! Handler for the `/info` endpoint.

use crate::{
    api::state::BrokerState,
    error::{AndromedaError, ResultInstanceExt},
};
use andromeda_http_extraction::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_ngsi_ld_model::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/info/sourceIdentity -- Retrieve context source identity.
pub async fn get(
    State(state): State<BrokerState>,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
) -> Result<Response, ProblemResponse> {
    let identity = ContextSourceIdentity {
        id: state.source_id,
        type_: ContextSourceIdentityType,
        context_source_alias: state.source_alias,
    };
    let context = context.into_inner();
    let value = serde_json::to_value(&identity)
        .map_err(|e| AndromedaError::internal(format!("Failed to serialize context source identity: {}", e)).with_instance(&uri))?;
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .with_instance(&uri)?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
