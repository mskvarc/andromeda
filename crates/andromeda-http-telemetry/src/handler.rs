//! Metrics scrape operation.

#![expect(clippy::absolute_paths, reason = "clippy reads a nested item of a grouped import as an inline path")]

use crate::state::TelemetryRouteState;
use axum::{extract::State, http::header::CONTENT_TYPE, response::IntoResponse};

/// Return every recorded series in the Prometheus text exposition format.
pub async fn scrape(State(state): State<TelemetryRouteState>) -> impl IntoResponse {
    ([(CONTENT_TYPE, state.content_type().clone())], state.recorder().render())
}
