use crate::{
    api::state::AppState,
    error::{AndromedaError, api::ApiError},
    http::{
        extractor::{accept::AcceptFormat, context::ContextUrl, prefer::PreferHeader},
        response::single::NgsiLdResponse,
    },
    renderer::response::ResponseRenderer,
};
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize)]
pub struct ContextSourceIdentity {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: &'static str,
}

pub async fn source_identity(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
) -> Result<Response, ApiError> {
    let identity = ContextSourceIdentity {
        id: state.source_identity,
        type_: "ContextSourceIdentity",
    };
    let context = context.into_inner();
    let value = serde_json::to_value(&identity)
        .map_err(|e| ApiError::from_error(AndromedaError::internal("Failed to serialize context source identity").with_source(e), uri))?;
    let rendered = ResponseRenderer::new(accept.format(), context, PreferHeader::default()).render_single(value);
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
