//! The exposition format the metrics route serves.

use crate::error::TelemetryHttpError;
use axum::http::HeaderValue;
use mediatype::MediaType;

/// The Prometheus text exposition format, version 0.0.4.
///
/// The `Content-Type` a Prometheus scraper expects from a text endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrometheusText;

impl PrometheusText {
    /// The media type this format is served as.
    ///
    /// The `version` parameter carries no registered constant in `mediatype`,
    /// so the type is validated by parsing once at construction rather than
    /// asserted by construction.
    fn media_type() -> Result<MediaType<'static>, TelemetryHttpError> {
        Ok(MediaType::parse("text/plain; version=0.0.4")?)
    }

    /// The `Content-Type` header value this format is served with.
    ///
    /// # Errors
    ///
    /// Returns [`TelemetryHttpError::MediaType`] when the exposition media type does not parse,
    /// and [`TelemetryHttpError::HeaderValue`] when its rendering is not a valid header value.
    pub fn content_type() -> Result<HeaderValue, TelemetryHttpError> {
        Ok(HeaderValue::from_str(&PrometheusText::media_type()?.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::TelemetryHttpError, exposition::PrometheusText};

    #[test]
    fn the_content_type_names_the_text_format_and_its_version() -> Result<(), TelemetryHttpError> {
        pretty_assertions::assert_eq!(PrometheusText::content_type()?.to_str().ok(), Some("text/plain; version=0.0.4"));
        Ok(())
    }
}
