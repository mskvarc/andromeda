//! The media type an NGSI-LD response is serialised as (clause 6.3.4).
//!
//! Distinct from the response format,
//! which names the clause 4.5.1 rendering of an entity. The two are chosen
//! independently: a `keyValues` entity can be answered as `application/json`,
//! `application/ld+json` or, where the endpoint allows it, `application/geo+json`.

use http::{HeaderName, HeaderValue};
use mediatype::{MediaType, MediaTypeBuf, Name, names};
use std::{fmt, iter};

/// The lowercase `Content-Type` field name this header is transmitted under.
static CONTENT_TYPE: HeaderName = HeaderName::from_static("content-type");

/// `application/json`
pub const APPLICATION_JSON: MediaType<'static> = MediaType::new(names::APPLICATION, names::JSON);

/// `application/ld+json`
pub const APPLICATION_LD_JSON: MediaType<'static> = MediaType::from_parts(names::APPLICATION, Name::new_unchecked("ld"), Some(names::JSON), &[]);

/// `application/geo+json`
pub const APPLICATION_GEO_JSON: MediaType<'static> = MediaType::from_parts(names::APPLICATION, Name::new_unchecked("geo"), Some(names::JSON), &[]);

/// The media type an NGSI-LD response body carries.
///
/// Per ETSI GS CIM 009 clause 6.3.4, the choice also decides where the
/// `@context` goes: `application/ld+json` carries it in the body, the other two
/// carry it in a `Link` header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseMediaType {
    /// `application/json` -- `@context` omitted from the body, provided via a `Link` header.
    #[default]
    Json,
    /// `application/ld+json` -- `@context` included in the body as a URL reference.
    JsonLd,
    /// `application/geo+json` -- `GeoJSON` `FeatureCollection` format (clause 6.3.15).
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
    ///
    /// Clause 6.3.4 asks that an `Accept` header "include (or define a media
    /// range that can be expanded to) at least one of" the three, so a range is
    /// read as the media type it expands to: `*/*` and `application/*` both
    /// cover all three, and "the order of the list above is significant" makes
    /// the first of them the one selected.
    #[must_use]
    pub fn from_media_type(media_type: &MediaType<'_>) -> Option<ResponseMediaType> {
        if media_type.ty == names::_STAR && media_type.subty == names::_STAR {
            // A client that will take anything gets the clause 6.3.4 default.
            return Some(ResponseMediaType::Json);
        }
        if media_type.ty != names::APPLICATION {
            return None;
        }
        if media_type.subty == names::_STAR {
            // `application/*` covers every one of the three, so it expands to
            // the first the clause lists.
            return Some(ResponseMediaType::Json);
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
    #[must_use]
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
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            ResponseMediaType::Json => "application/json",
            ResponseMediaType::JsonLd => "application/ld+json",
            ResponseMediaType::GeoJson => "application/geo+json",
        }
    }

    /// Where clause 6.3.4 places this media type in the list it calls
    /// significant, counting from the front.
    ///
    /// "The order of the list above is significant. If the Accept header can be
    /// expanded to more than one of the options of the list, the first one of
    /// the list shall be selected, unless amended by the HTTP Accept header
    /// processing rules, e.g. the presence of a "q" parameter indicating a
    /// relative weight [...] require otherwise." Two representations a client
    /// offered at the same weight are therefore separated by this precedence
    /// and not by the order the client happened to write them in.
    ///
    /// Read off the advertised list rather than restated, so the order a `406`
    /// publishes and the order negotiation selects in cannot drift apart.
    #[must_use]
    pub fn clause_precedence(self) -> usize {
        GeoJsonSupport::Allowed
            .available_representations()
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(usize::MAX)
    }
}

impl fmt::Display for ResponseMediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl headers::Header for ResponseMediaType {
    fn name() -> &'static HeaderName {
        &CONTENT_TYPE
    }
    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let parsed = value
            .to_str()
            .map_err(|_| headers::Error::invalid())?
            .parse::<MediaTypeBuf>()
            .map_err(|_| headers::Error::invalid())?;
        let parsed_ref: MediaType<'_> = (&parsed).into();
        Self::from_media_type(&parsed_ref).ok_or_else(headers::Error::invalid)
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        values.extend(iter::once(HeaderValue::from_static(self.as_str())));
    }
}

/// Whether an endpoint may answer with `GeoJSON`.
///
/// Clause 6.3.15 defines the `GeoJSON` representation for entity resources only;
/// the temporal, subscription and registration resources have no `GeoJSON` form,
/// so their endpoints answer `406` rather than negotiating one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoJsonSupport {
    /// The endpoint supports `application/geo+json` responses.
    Allowed,
    /// The endpoint does not support `GeoJSON` responses.
    Disallowed,
}

impl GeoJsonSupport {
    /// The representations an endpoint with this support can answer with.
    ///
    /// Clause 6.3.4 lists the acceptable media types as `application/json`,
    /// `application/ld+json` and `application/geo+json` and states that "the
    /// order of the list above is significant", so the list keeps that order:
    /// it is the same list a `406` response publishes as "the list of the
    /// available representations of the resources", and a client reading it
    /// top-down picks what the broker would have picked. `GeoJSON` is absent
    /// for an endpoint that does not offer it, which is what makes such an
    /// endpoint's `406` truthful rather than an invitation to retry with a
    /// media type it would refuse again.
    #[must_use]
    pub const fn available_representations(self) -> &'static [ResponseMediaType] {
        match self {
            GeoJsonSupport::Allowed => &[ResponseMediaType::Json, ResponseMediaType::JsonLd, ResponseMediaType::GeoJson],
            GeoJsonSupport::Disallowed => &[ResponseMediaType::Json, ResponseMediaType::JsonLd],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::media_type::{APPLICATION_GEO_JSON, APPLICATION_JSON, APPLICATION_LD_JSON, GeoJsonSupport, ResponseMediaType};
    use mediatype::{MediaType, names};

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
    fn display_matches_the_content_type() {
        pretty_assertions::assert_eq!(ResponseMediaType::JsonLd.to_string(), "application/ld+json");
    }

    /// Clause 6.3.4: `GeoJSON` is acceptable for the entity consumption
    /// operations, so only an endpoint that allows it advertises it.
    #[test]
    fn only_a_geojson_endpoint_advertises_the_geojson_representation() {
        pretty_assertions::assert_eq!(
            GeoJsonSupport::Allowed.available_representations(),
            [ResponseMediaType::Json, ResponseMediaType::JsonLd, ResponseMediaType::GeoJson]
        );
        pretty_assertions::assert_eq!(
            GeoJsonSupport::Disallowed.available_representations(),
            [ResponseMediaType::Json, ResponseMediaType::JsonLd]
        );
    }

    /// Clause 6.3.4: "the order of the list above is significant", the list
    /// being `application/json`, `application/ld+json`, `application/geo+json`.
    #[test]
    fn the_advertised_representations_keep_the_clause_order() {
        for support in [GeoJsonSupport::Allowed, GeoJsonSupport::Disallowed] {
            let rendered: Vec<&str> = support.available_representations().iter().map(ResponseMediaType::as_str).collect();
            pretty_assertions::assert_eq!(rendered[0], "application/json");
            pretty_assertions::assert_eq!(rendered[1], "application/ld+json");
        }
    }

    /// The same significant order is what separates two representations a
    /// client offered at the same weight, so it is readable as a precedence
    /// rather than only as a list to print.
    #[test]
    fn the_clause_precedence_runs_json_then_json_ld_then_geojson() {
        assert!(ResponseMediaType::Json.clause_precedence() < ResponseMediaType::JsonLd.clause_precedence());
        assert!(ResponseMediaType::JsonLd.clause_precedence() < ResponseMediaType::GeoJson.clause_precedence());
    }

    /// Clause 6.3.4: an `Accept` may "define a media range that can be expanded
    /// to" one of the three. `application/*` covers all three, so it expands to
    /// the first the clause lists rather than being refused with a 406.
    #[test]
    fn an_application_media_range_expands_to_the_first_representation() {
        let range: MediaType<'static> = MediaType::new(names::APPLICATION, names::_STAR);
        pretty_assertions::assert_eq!(ResponseMediaType::from_media_type(&range), Some(ResponseMediaType::Json));
    }

    /// A range under another top-level type covers none of the three.
    #[test]
    fn a_media_range_under_another_type_expands_to_no_representation() {
        let range: MediaType<'static> = MediaType::new(names::TEXT, names::_STAR);
        pretty_assertions::assert_eq!(ResponseMediaType::from_media_type(&range), None);
    }
}
