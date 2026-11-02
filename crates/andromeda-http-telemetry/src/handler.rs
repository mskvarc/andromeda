//! Metrics scrape operation.

use crate::state::TelemetryRouteState;
use andromeda_telemetry::http::UnmeteredResponse;
use axum::{Extension, extract::State, http::header::CONTENT_TYPE, response::IntoResponse};

/// Return every recorded series in the Prometheus text exposition format,
/// marked so the request metrics do not count the scrape itself.
pub async fn scrape(State(state): State<TelemetryRouteState>) -> impl IntoResponse {
    (
        Extension(UnmeteredResponse),
        // clone: the header value is a shared, immutable byte string handed to each response.
        [(CONTENT_TYPE, state.content_type().clone())],
        state.recorder().render(),
    )
}
