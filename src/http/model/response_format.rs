//! Response format types for NGSI-LD content negotiation.

use mediatype::MediaType;
use std::fmt;

/// `application/json`
pub const APPLICATION_JSON: MediaType<'static> = MediaType::new(mediatype::names::APPLICATION, mediatype::names::JSON);

/// `application/ld+json`
pub const APPLICATION_LD_JSON: MediaType<'static> = MediaType::from_parts(
    mediatype::names::APPLICATION,
    mediatype::Name::new_unchecked("ld"),
    Some(mediatype::names::JSON),
    &[],
);

/// `application/geo+json`
pub const APPLICATION_GEO_JSON: MediaType<'static> = MediaType::from_parts(
    mediatype::names::APPLICATION,
    mediatype::Name::new_unchecked("geo"),
    Some(mediatype::names::JSON),
    &[],
);

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
    /// Get the structured [`MediaType`] for this format.
    pub const fn content_type(&self) -> MediaType<'static> {
        match self {
            ResponseFormat::Json => APPLICATION_JSON,
            ResponseFormat::JsonLd => APPLICATION_LD_JSON,
            ResponseFormat::GeoJson => APPLICATION_GEO_JSON,
        }
    }

    /// Get the Content-Type string for this format.
    pub const fn content_type_str(&self) -> &'static str {
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
        write!(f, "{}", self.content_type())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn response_format_content_type_str() {
        pretty_assertions::assert_eq!(ResponseFormat::Json.content_type_str(), "application/json");
        pretty_assertions::assert_eq!(ResponseFormat::JsonLd.content_type_str(), "application/ld+json");
        pretty_assertions::assert_eq!(ResponseFormat::GeoJson.content_type_str(), "application/geo+json");
    }

    #[test]
    fn response_format_content_type_media_type() {
        pretty_assertions::assert_eq!(ResponseFormat::Json.content_type(), APPLICATION_JSON);
        pretty_assertions::assert_eq!(ResponseFormat::JsonLd.content_type(), APPLICATION_LD_JSON);
        pretty_assertions::assert_eq!(ResponseFormat::GeoJson.content_type(), APPLICATION_GEO_JSON);
    }

    #[test]
    fn response_format_include_context_in_body() {
        assert!(!ResponseFormat::Json.include_context_in_body());
        assert!(ResponseFormat::JsonLd.include_context_in_body());
        assert!(!ResponseFormat::GeoJson.include_context_in_body());
    }
}
