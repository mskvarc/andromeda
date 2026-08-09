//! The media type an NGSI-LD response is serialised as (clause 6.3.3).
//!
//! Distinct from [`ResponseFormat`](andromeda_ngsi_ld_representation::response_format::ResponseFormat),
//! which names the clause 4.5.1 rendering of an entity. The two are chosen
//! independently: a `keyValues` entity can be answered as `application/json`,
//! `application/ld+json` or, where the endpoint allows it, `application/geo+json`.

use mediatype::{MediaType, Name, names};
use std::fmt;

/// `application/json`
pub const APPLICATION_JSON: MediaType<'static> = MediaType::new(names::APPLICATION, names::JSON);

/// `application/ld+json`
pub const APPLICATION_LD_JSON: MediaType<'static> = MediaType::from_parts(names::APPLICATION, Name::new_unchecked("ld"), Some(names::JSON), &[]);

/// `application/geo+json`
pub const APPLICATION_GEO_JSON: MediaType<'static> = MediaType::from_parts(names::APPLICATION, Name::new_unchecked("geo"), Some(names::JSON), &[]);

/// The media type an NGSI-LD response body carries.
///
/// Per ETSI GS CIM 009 clause 6.3.3, the choice also decides where the
/// `@context` goes: `application/ld+json` carries it in the body, the other two
/// carry it in a `Link` header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseMediaType {
    /// `application/json` -- `@context` omitted from the body, provided via a `Link` header.
    #[default]
    Json,
    /// `application/ld+json` -- `@context` included in the body as a URL reference.
    JsonLd,
    /// `application/geo+json` -- GeoJSON `FeatureCollection` format (clause 6.3.8).
    GeoJson,
}

impl ResponseMediaType {
    /// Which of the three NGSI-LD media types a client spelled, if any.
    ///
    /// The same question is asked in two places and must be answered the same
    /// way: negotiating a response from an `Accept` header, and deciding what
    /// a notification is serialised as from a subscription's
    /// `notification.endpoint.accept` (clause 5.2.15). Parameters are ignored,
    /// so a `;q=` or a `;charset=` does not stop a media type being
    /// recognised.
    ///
    /// `application/json` is matched with no suffix on purpose:
    /// `application/merge-patch+json` and `application/json-patch+json` are
    /// request body types, not response types, and must not negotiate as JSON.
    pub fn from_media_type(media_type: &MediaType<'_>) -> Option<ResponseMediaType> {
        if media_type.ty == names::_STAR && media_type.subty == names::_STAR {
            // A client that will take anything gets the clause 6.3.3 default.
            return Some(ResponseMediaType::Json);
        }
        if media_type.ty != names::APPLICATION {
            return None;
        }
        // `Name` is not usable in a pattern, so this is an if-chain rather
        // than the match its shape wants.
        if media_type.subty == names::LD && media_type.suffix == Some(names::JSON) {
            return Some(ResponseMediaType::JsonLd);
        }
        if media_type.subty == names::GEO && media_type.suffix == Some(names::JSON) {
            return Some(ResponseMediaType::GeoJson);
        }
        if media_type.subty == names::JSON && media_type.suffix.is_none() {
            return Some(ResponseMediaType::Json);
        }
        None
    }

    /// The structured [`MediaType`] this response carries.
    pub const fn media_type(&self) -> MediaType<'static> {
        match self {
            ResponseMediaType::Json => APPLICATION_JSON,
            ResponseMediaType::JsonLd => APPLICATION_LD_JSON,
            ResponseMediaType::GeoJson => APPLICATION_GEO_JSON,
        }
    }

    /// The `Content-Type` value as a static string.
    ///
    /// Rendering [`media_type`](Self::media_type) would produce the same text at
    /// the cost of an allocation on every response.
    pub const fn as_str(&self) -> &'static str {
        match self {
            ResponseMediaType::Json => "application/json",
            ResponseMediaType::JsonLd => "application/ld+json",
            ResponseMediaType::GeoJson => "application/geo+json",
        }
    }

    /// Whether the `@context` belongs in the response body.
    pub const fn include_context_in_body(&self) -> bool {
        matches!(self, ResponseMediaType::JsonLd)
    }
}

impl fmt::Display for ResponseMediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an endpoint may answer with GeoJSON.
///
/// Clause 6.3.8 defines the GeoJSON representation for entity resources only;
/// the temporal, subscription and registration resources have no GeoJSON form,
/// so their endpoints answer `406` rather than negotiating one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoJsonSupport {
    /// The endpoint supports `application/geo+json` responses.
    Allowed,
    /// The endpoint does not support GeoJSON responses.
    Disallowed,
}

#[cfg(test)]
mod tests {
    use crate::media_type::{APPLICATION_GEO_JSON, APPLICATION_JSON, APPLICATION_LD_JSON, ResponseMediaType};

    #[test]
    fn each_media_type_renders_its_content_type() {
        pretty_assertions::assert_eq!(ResponseMediaType::Json.as_str(), "application/json");
        pretty_assertions::assert_eq!(ResponseMediaType::JsonLd.as_str(), "application/ld+json");
        pretty_assertions::assert_eq!(ResponseMediaType::GeoJson.as_str(), "application/geo+json");
    }

    #[test]
    fn the_static_string_and_the_structured_media_type_agree() {
        for (variant, media_type) in [
            (ResponseMediaType::Json, APPLICATION_JSON),
            (ResponseMediaType::JsonLd, APPLICATION_LD_JSON),
            (ResponseMediaType::GeoJson, APPLICATION_GEO_JSON),
        ] {
            pretty_assertions::assert_eq!(variant.media_type(), media_type);
            pretty_assertions::assert_eq!(variant.as_str(), media_type.to_string());
        }
    }

    #[test]
    fn only_json_ld_carries_the_context_in_the_body() {
        assert!(!ResponseMediaType::Json.include_context_in_body());
        assert!(ResponseMediaType::JsonLd.include_context_in_body());
        assert!(!ResponseMediaType::GeoJson.include_context_in_body());
    }

    #[test]
    fn display_matches_the_content_type() {
        pretty_assertions::assert_eq!(ResponseMediaType::JsonLd.to_string(), "application/ld+json");
    }
}
