//! Focused metrics-route state.

use andromeda_telemetry::recorder::TelemetryRecorder;
use axum::http::HeaderValue;
use std::sync::Arc;

/// State containing only the capability to render recorded metrics.
#[derive(Clone)]
pub struct TelemetryRouteState {
    /// The installed recorder whose contents the route renders.
    recorder: Arc<TelemetryRecorder>,
    /// The `Content-Type` every scrape response carries.
    content_type: HeaderValue,
}

impl TelemetryRouteState {
    /// Construct focused metrics-route state.
    pub const fn new(recorder: Arc<TelemetryRecorder>, content_type: HeaderValue) -> TelemetryRouteState {
        TelemetryRouteState { recorder, content_type }
    }

    /// Borrow the recorder.
    pub fn recorder(&self) -> &TelemetryRecorder {
        &self.recorder
    }

    /// Borrow the scrape response content type.
    pub const fn content_type(&self) -> &HeaderValue {
        &self.content_type
    }
}
