//! Handler for the `/info` endpoint.

use crate::{
    api::state::BrokerState,
    error::AndromedaError,
    http::{
        error::problem::NgsiLdError,
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader},
        response::{render::start_rendering, single::NgsiLdResponse},
    },
};
use andromeda_ngsi_ld_model::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};

/// GET /ngsi-ld/v1/info/sourceIdentity -- Retrieve context source identity.
pub async fn get(State(state): State<BrokerState>, OriginalUri(uri): OriginalUri, accept: AcceptFormat, context: ContextUrl) -> Result<Response, NgsiLdError> {
    let identity = ContextSourceIdentity {
        id: state.source_id,
        type_: ContextSourceIdentityType,
        context_source_alias: state.source_alias,
    };
    let context = context.into_inner();
    let value = serde_json::to_value(&identity)
        .map_err(|e| AndromedaError::internal(format!("Failed to serialize context source identity: {}", e)).with_instance(&uri))?;
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
        .with_payload(ResponsePayload::Single(value))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
