//! Metrics scrape operation.

use crate::state::TelemetryRouteState;
use axum::{extract::State, http::header::CONTENT_TYPE, response::IntoResponse};

/// Return every recorded series in the Prometheus text exposition format.
pub async fn scrape(State(state): State<TelemetryRouteState>) -> impl IntoResponse {
    // clone: the header value is a shared, immutable byte string handed to each response.
    ([(CONTENT_TYPE, state.content_type().clone())], state.recorder().render())
}
