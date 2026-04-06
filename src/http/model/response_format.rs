//! Response format types for NGSI-LD content negotiation.

use std::fmt;

/// Response format based on Accept header.
///
/// Per ETSI GS CIM 009:
/// - `application/json`: Entity without @context, context URL in Link header
/// - `application/ld+json`: Entity with @context as URL reference in body
/// - `application/geo+json`: GeoJSON FeatureCollection format
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseFormat {
    /// `application/json` -- @context omitted from body, provided via Link header.
    #[default]
    Json,
    /// `application/ld+json` -- @context included in body as URL reference.
    JsonLd,
    /// `application/geo+json` -- GeoJSON FeatureCollection format.
    GeoJson,
}

impl ResponseFormat {
    /// Get the Content-Type header value for this format.
    pub const fn content_type(&self) -> &'static str {
        match self {
            ResponseFormat::Json => "application/json",
            ResponseFormat::JsonLd => "application/ld+json",
            ResponseFormat::GeoJson => "application/geo+json",
        }
    }

    /// Whether @context should be included in the response body.
    pub const fn include_context_in_body(&self) -> bool {
        matches!(self, ResponseFormat::JsonLd)
    }
}

impl fmt::Display for ResponseFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.content_type())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_format_content_type() {
        assert_eq!(ResponseFormat::Json.content_type(), "application/json");
        assert_eq!(ResponseFormat::JsonLd.content_type(), "application/ld+json");
        assert_eq!(ResponseFormat::GeoJson.content_type(), "application/geo+json");
    }

    #[test]
    fn response_format_include_context_in_body() {
        assert!(!ResponseFormat::Json.include_context_in_body());
        assert!(ResponseFormat::JsonLd.include_context_in_body());
        assert!(!ResponseFormat::GeoJson.include_context_in_body());
    }
}
