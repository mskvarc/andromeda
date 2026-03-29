use crate::api::{
    error::ApiError,
    extractor::{accept::AcceptFormat, context::ContextUrl},
    response::ngsild::NgsiLdResponse,
    state::AppState,
};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize)]
pub struct ContextSourceIdentity {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: &'static str,
}

pub async fn source_identity(State(state): State<AppState>, accept: AcceptFormat, context: ContextUrl) -> Result<Response, ApiError> {
    let identity = ContextSourceIdentity {
        id: state.source_identity.clone(),
        type_: "ContextSourceIdentity",
    };
    let context = context.into_inner();
    Ok(NgsiLdResponse::ok(identity).with_context(context).with_format(accept.format()).into_response())
}
