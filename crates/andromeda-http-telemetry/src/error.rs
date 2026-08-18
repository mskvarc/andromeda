//! Failures raised while constructing the metrics route.

use axum::http::header::InvalidHeaderValue;
use mediatype::MediaTypeError;
use thiserror::Error;

/// Why the metrics route could not be constructed.
#[derive(Debug, Error)]
pub enum TelemetryHttpError {
    /// The exposition media type could not be parsed.
    #[error("metrics exposition media type is invalid: {0}")]
    MediaType(#[from] MediaTypeError),
    /// The exposition media type does not render as a valid header value.
    #[error("metrics exposition content type is not a valid header value: {0}")]
    HeaderValue(#[from] InvalidHeaderValue),
}
