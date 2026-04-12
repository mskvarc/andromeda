//! Typed `Link` header for NGSI-LD (RFC 8288, ETSI GS CIM 009 clauses 6.3.5, 6.3.6, 6.3.10).
//!
//! Portions of this file are derived from [Headers](https://github.com/hyperium/headers).
//! Licensed under the MIT License.
//! Copyright (c) 2014-2025 Sean McArthur.
//!
//! Implements the subset of the HTTP `Link` header
//! ([RFC 8288](https://datatracker.ietf.org/doc/html/rfc8288)) required by NGSI-LD:
//!
//! - **JSON-LD `@context` links** (clause 6.3.5): `rel="http://www.w3.org/ns/json-ld#context"`
//!   with `type="application/ld+json"`, used to associate a JSON-LD context with requests
//!   and responses.
//! - **Pagination links** (clause 6.3.10): `rel="next"` / `rel="prev"` with a `type`
//!   parameter matching the negotiated media type.
//! - **Notification context links** (clauses 6.3.8, 6.3.9): same structure as `@context`
//!   links, included in HTTP notification POST requests.
//!
//! Only the `rel` and `type` link-param attributes are supported. Other RFC 8288 parameters
//! (`anchor`, `rev`, `hreflang`, `media`, `title`, `title*`) are not used by NGSI-LD and
//! are silently ignored during parsing for interoperability.

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use headers::{Header, HeaderMapExt};
use mediatype::MediaTypeBuf;
use std::{borrow::Cow, fmt, str::FromStr};

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

/// Typed `Link` header (RFC 8288).
///
/// Contains one or more [`LinkValue`]s, each representing a single `link-value`
/// within the header.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Link {
    values: Vec<LinkValue>,
}

impl Link {
    /// Create a `Link` from a `Vec<LinkValue>`.
    pub const fn new(link_values: Vec<LinkValue>) -> Link {
        Link { values: link_values }
    }

    /// Return the contained `LinkValue`s.
    pub fn values(&self) -> &[LinkValue] {
        &self.values
    }

    /// Build a `Link` from a list of `LinkValue`s, returning `None` if empty.
    pub fn from_values(values: Vec<LinkValue>) -> Option<Link> {
        if values.is_empty() { None } else { Some(Link::new(values)) }
    }

    /// Create a single-value JSON-LD context `Link` header.
    pub fn jsonld_context(context_url: &str) -> Link {
        Link::new(vec![LinkValue::jsonld_context(context_url)])
    }

    /// Create a multi-value JSON-LD context `Link` header from an ordered URL list.
    fn jsonld_contexts(urls: &[String]) -> Option<Link> {
        let values = urls.iter().map(|url| LinkValue::jsonld_context(url)).collect::<Vec<_>>();
        Link::from_values(values)
    }

    /// Insert JSON-LD context links into a header map as a single `Link` header.
    pub fn insert_jsonld_contexts(headers: &mut HeaderMap, urls: &[String]) {
        if let Some(link) = Link::jsonld_contexts(urls) {
            headers.typed_insert(link);
        }
    }

    /// Encode this `Link` header into a raw `HeaderValue`.
    ///
    /// Returns `None` if encoding yields no values.
    pub fn to_header_value(&self) -> Option<HeaderValue> {
        let mut values = Vec::new();
        self.encode(&mut values);
        if values.is_empty() {
            return None;
        }
        if values.len() == 1 {
            return values.into_iter().next();
        }
        let mut combined = String::new();
        for (idx, value) in values.iter().enumerate() {
            let part = value.to_str().ok()?;
            if idx > 0 {
                combined.push_str(", ");
            }
            combined.push_str(part);
        }
        HeaderValue::from_str(&combined).ok()
    }

    /// Encode this `Link` header into a string, if possible.
    pub fn to_header_string(&self) -> Option<String> {
        self.to_header_value().and_then(|v| v.to_str().ok().map(|s| s.to_string()))
    }

    /// Return target URIs of all JSON-LD context link-values.
    pub fn context_urls(&self) -> impl Iterator<Item = &str> {
        self.values().iter().filter(|lv| lv.is_context_link()).map(|lv| lv.link())
    }
}

// ---------------------------------------------------------------------------
// LinkValue
// ---------------------------------------------------------------------------

/// A single `link-value` within a `Link` header (RFC 8288 section 3).
///
/// Only carries the parameters used by NGSI-LD: `rel` (forward relation types)
/// and `type` (media type hint). All other RFC 8288 parameters are discarded
/// during parsing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkValue {
    /// Target IRI.
    link: Cow<'static, str>,
    /// Forward relation types: `rel`.
    rel: Option<Vec<RelationType>>,
    /// Hint on the media type of the result: `type`.
    media_type: Option<MediaTypeBuf>,
}

impl LinkValue {
    /// `application/ld+json` media type for Link header `type` parameter.
    pub const JSON_LD_MEDIA_TYPE: &str = "application/ld+json";

    /// Create a `LinkValue` from a URI-Reference.
    pub fn new<T>(uri: T) -> LinkValue
    where
        T: Into<Cow<'static, str>>,
    {
        LinkValue {
            link: uri.into(),
            rel: None,
            media_type: None,
        }
    }

    /// Return the target IRI of this link-value.
    pub fn link(&self) -> &str {
        self.link.as_ref()
    }

    /// Return the `rel` parameter(s), if present.
    pub fn rel(&self) -> Option<&[RelationType]> {
        self.rel.as_deref()
    }

    /// Add a [`RelationType`] to the `rel` parameter.
    pub fn push_rel(mut self, rel: RelationType) -> LinkValue {
        self.rel.get_or_insert_with(Vec::new).push(rel);
        self
    }

    /// Set the `type` parameter.
    pub fn set_media_type(mut self, media_type: MediaTypeBuf) -> LinkValue {
        self.media_type = Some(media_type);
        self
    }

    /// Build a typed `LinkValue` with optional `rel` and `type` parameters.
    pub fn with_params(target: &str, rel: Option<&str>, media_type: Option<&str>) -> LinkValue {
        let mut value = LinkValue::new(target.to_owned());
        if let Some(rel) = rel {
            // RelationType::from_str returns Infallible, so Ok is always produced.
            match rel.parse::<RelationType>() {
                Ok(rel) => value = value.push_rel(rel),
                Err(infallible) => match infallible {},
            }
        }
        if let Some(media_type) = media_type
            && let Ok(parsed) = media_type.parse::<MediaTypeBuf>()
        {
            value = value.set_media_type(parsed);
        }
        value
    }

    /// Build a JSON-LD context link-value with the standard relation type and media type.
    pub fn jsonld_context(target: &str) -> LinkValue {
        let mut lv = LinkValue::new(target.to_owned()).push_rel(RelationType::jsonld_context());
        if let Ok(mt) = Self::JSON_LD_MEDIA_TYPE.parse::<MediaTypeBuf>() {
            lv = lv.set_media_type(mt);
        }
        lv
    }

    /// Returns true if this is a JSON-LD context link.
    pub fn is_context_link(&self) -> bool {
        self.rel().is_some_and(|rels| rels.iter().any(RelationType::is_jsonld_context))
    }
}

// ---------------------------------------------------------------------------
// RelationType
// ---------------------------------------------------------------------------

/// Link Relation Type ([RFC 8288 section 2.1](https://datatracker.ietf.org/doc/html/rfc8288#section-2.1)).
///
/// Only relation types actually used by NGSI-LD are given named variants.
/// All others (registered or extension) are captured in [`ExtRelType`](RelationType::ExtRelType).
///
/// Named variants used by NGSI-LD:
/// - [`Next`](RelationType::Next) -- pagination (clause 6.3.10)
/// - [`Prev`](RelationType::Prev) -- pagination (clause 6.3.10)
/// - [`First`](RelationType::First) -- pagination (clause 6.3.10)
/// - [`Last`](RelationType::Last) -- pagination (clause 6.3.10)
///
/// The JSON-LD context relation (`http://www.w3.org/ns/json-ld#context`) is an
/// extension URI and is represented as `ExtRelType`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RelationType {
    /// `next` -- next page in a paginated result (clause 6.3.10).
    Next,
    /// `prev` -- previous page in a paginated result (clause 6.3.10).
    Prev,
    /// `first` -- first page in a paginated result (clause 6.3.10).
    First,
    /// `last` -- last page in a paginated result (clause 6.3.10).
    Last,
    /// Any other relation type (registered name or extension URI).
    ExtRelType(String),
}

impl RelationType {
    /// The JSON-LD context relation IRI.
    pub const JSONLD_CONTEXT: &str = "http://www.w3.org/ns/json-ld#context";

    /// Create the JSON-LD context relation type.
    pub fn jsonld_context() -> RelationType {
        RelationType::ExtRelType(Self::JSONLD_CONTEXT.to_owned())
    }

    /// Returns true if this is the JSON-LD context relation type.
    pub fn is_jsonld_context(&self) -> bool {
        matches!(self, RelationType::ExtRelType(uri) if uri == Self::JSONLD_CONTEXT)
    }
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationType::Next => f.write_str("next"),
            RelationType::Prev => f.write_str("prev"),
            RelationType::First => f.write_str("first"),
            RelationType::Last => f.write_str("last"),
            RelationType::ExtRelType(uri) => f.write_str(uri),
        }
    }
}

impl FromStr for RelationType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<RelationType, Self::Err> {
        Ok(if s.eq_ignore_ascii_case("next") {
            RelationType::Next
        } else if s.eq_ignore_ascii_case("prev") {
            RelationType::Prev
        } else if s.eq_ignore_ascii_case("first") {
            RelationType::First
        } else if s.eq_ignore_ascii_case("last") {
            RelationType::Last
        } else {
            RelationType::ExtRelType(String::from(s))
        })
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, value) in self.values.iter().enumerate() {
            if idx > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{value}")?;
        }
        Ok(())
    }
}

impl fmt::Display for LinkValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.link)?;

        if let Some(ref rel) = self.rel {
            f.write_str("; rel=\"")?;
            for (idx, r) in rel.iter().enumerate() {
                if idx > 0 {
                    f.write_str(" ")?;
                }
                write!(f, "{r}")?;
            }
            f.write_str("\"")?;
        }
        if let Some(ref media_type) = self.media_type {
            write!(f, "; type=\"{media_type}\"")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

impl FromStr for Link {
    type Err = LinkParseError;

    fn from_str(s: &str) -> Result<Link, LinkParseError> {
        let splitter = SplitAsciiUnquoted::new(s, ";,");

        let mut link_values: Vec<LinkValue> = Vec::new();

        for segment in splitter {
            let trimmed = segment.trim();
            if trimmed.starts_with('<') {
                let target = verify_and_trim(trimmed, b'<', b'>')?;
                link_values.push(LinkValue::new(target.to_owned()));
            } else {
                let mut param_split = segment.splitn(2, '=');

                let param_name = match param_split.next() {
                    Some(p) if !p.trim().is_empty() => p.trim(),
                    _ => return Err(LinkParseError),
                };

                let link_header = link_values.last_mut().ok_or(LinkParseError)?;

                if "rel".eq_ignore_ascii_case(param_name) {
                    if link_header.rel.is_none() {
                        link_header.rel = Some(parse_relation_types(param_split.next())?);
                    }
                } else if "type".eq_ignore_ascii_case(param_name) {
                    if link_header.media_type.is_none() {
                        let raw = param_split.next().filter(|s| !s.is_empty()).ok_or(LinkParseError)?;
                        let unquoted = verify_and_trim(raw.trim(), b'"', b'"')?;
                        let mt: MediaTypeBuf = unquoted.parse().map_err(|_| LinkParseError)?;
                        link_header.media_type = Some(mt);
                    }
                } else {
                    // Silently ignore unknown parameters for interoperability
                    // (RFC 8288 section 3.4.1: unrecognised parameters are ignored).
                }
            }
        }

        Ok(Link::new(link_values))
    }
}

/// Parse relation types from a raw parameter value.
fn parse_relation_types(raw: Option<&str>) -> Result<Vec<RelationType>, LinkParseError> {
    let raw = raw.filter(|s| !s.is_empty()).ok_or(LinkParseError)?;
    raw.trim_matches(|c: char| c == '"' || c.is_whitespace())
        .split(' ')
        .filter(|t| !t.is_empty())
        .map(|t| {
            // RelationType::from_str is infallible
            t.trim().parse::<RelationType>().map_err(|_| LinkParseError)
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Error returned when a `Link` header cannot be parsed.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid Link header")]
pub struct LinkParseError;

// ---------------------------------------------------------------------------
// headers::Header impl
// ---------------------------------------------------------------------------

static LINK: HeaderName = HeaderName::from_static("link");

impl headers::Header for Link {
    fn name() -> &'static HeaderName {
        &LINK
    }

    fn decode<'i, I>(values: &mut I) -> Result<Link, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut combined: Option<Link> = None;
        for value in values {
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            let parsed: Link = s.parse().map_err(|_| headers::Error::invalid())?;
            match combined {
                None => combined = Some(parsed),
                Some(ref mut existing) => existing.values.extend(parsed.values),
            }
        }
        combined.ok_or_else(headers::Error::invalid)
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(&self.to_string()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode Link header"),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing utilities
// ---------------------------------------------------------------------------

/// Iterator that splits a string by the given delimiter characters, but ignores
/// delimiters inside double-quoted segments.
struct SplitAsciiUnquoted<'a> {
    src: &'a str,
    pos: usize,
    del: &'a str,
}

impl<'a> SplitAsciiUnquoted<'a> {
    const fn new(s: &'a str, d: &'a str) -> SplitAsciiUnquoted<'a> {
        SplitAsciiUnquoted { src: s, pos: 0, del: d }
    }
}

impl<'a> Iterator for SplitAsciiUnquoted<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.pos >= self.src.len() {
            return None;
        }
        let prev_pos = self.pos;
        let mut pos = self.pos;
        let mut in_quotes = false;

        for c in self.src.as_bytes()[prev_pos..].iter() {
            in_quotes ^= *c == b'"';
            if !in_quotes && self.del.as_bytes().contains(c) {
                break;
            }
            pos += 1;
        }

        self.pos = pos + 1;
        Some(&self.src[prev_pos..pos])
    }
}

/// Verify that `s` starts with `open` and ends with `close`, returning the inner
/// substring with those delimiters and surrounding whitespace trimmed.
fn verify_and_trim(s: &str, open: u8, close: u8) -> Result<&str, LinkParseError> {
    let bytes = s.as_bytes();
    if bytes.len() > 1 && bytes[0] == open && bytes[bytes.len() - 1] == close {
        Ok(s.trim_matches(|c: char| c == open as char || c == close as char || c.is_whitespace()))
    } else {
        Err(LinkParseError)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    impl LinkValue {
        /// Returns true if this link-value has the given relation type.
        fn has_rel(&self, rel: &RelationType) -> bool {
            self.rel().is_some_and(|rels| rels.contains(rel))
        }
    }

    impl Link {
        /// Return link-values matching a given relation type.
        fn find_by_rel(&self, rel: RelationType) -> impl Iterator<Item = &LinkValue> {
            self.values().iter().filter(move |lv| lv.has_rel(&rel))
        }
    }
    use headers::Header;

    /// Decode a `Link` from a raw header string using the `headers::Header` trait.
    fn decode_link(raw: &str) -> Option<Link> {
        let val = HeaderValue::from_str(raw).ok()?;
        let mut iter = std::iter::once(&val);
        Link::decode(&mut iter).ok()
    }

    #[test]
    fn parse_single_link_with_rel() {
        let expected = Link::new(vec![
            LinkValue::new("http://example.com/TheBook/chapter2")
                .push_rel(RelationType::Prev)
                .set_media_type("text/html".parse::<MediaTypeBuf>().unwrap()),
        ]);

        let raw = r#"<http://example.com/TheBook/chapter2>; rel="prev"; type="text/html""#;
        let parsed = decode_link(raw);
        assert_eq!(parsed, Some(expected));
    }

    #[test]
    fn parse_multiple_link_values() {
        let first = LinkValue::new("/page/2").push_rel(RelationType::Next);
        let second = LinkValue::new("/page/0").push_rel(RelationType::Prev);

        let raw = r#"</page/2>; rel="next", </page/0>; rel="prev""#;
        let expected = Link::new(vec![first, second]);
        let parsed = decode_link(raw);
        assert_eq!(parsed, Some(expected));
    }

    #[test]
    fn parse_jsonld_context_link() {
        let raw = r#"<http://json-ld.org/contexts/person.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;

        let expected = Link::new(vec![
            LinkValue::new("http://json-ld.org/contexts/person.jsonld")
                .push_rel(RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".to_owned()))
                .set_media_type("application/ld+json".parse::<MediaTypeBuf>().unwrap()),
        ]);

        let parsed = decode_link(raw).expect("should parse");
        assert_eq!(parsed, expected);

        // Verify accessors
        let values = parsed.values();
        assert_eq!(values[0].link(), "http://json-ld.org/contexts/person.jsonld");
        let rels = values[0].rel().expect("should have rel");
        assert_eq!(rels[0].to_string(), "http://www.w3.org/ns/json-ld#context");
    }

    #[test]
    fn unknown_parameters_are_silently_ignored() {
        // RFC 8288 section 3.4.1: unrecognized parameters should be ignored.
        // This also ensures interoperability with brokers that send extra params.
        let raw = r#"<http://example.com/>; rel="next"; title="Chapter 2"; hreflang=en"#;
        let parsed = decode_link(raw).expect("should parse despite unknown params");
        assert_eq!(parsed.values().len(), 1);
        assert_eq!(parsed.values()[0].link(), "http://example.com/");
    }

    #[test]
    fn link_display_with_rel_and_type() {
        let lv = LinkValue::new("http://example.com/ctx.jsonld")
            .push_rel(RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".to_owned()))
            .set_media_type("application/ld+json".parse::<MediaTypeBuf>().unwrap());

        let link = Link::new(vec![lv]);

        let mut header_str = String::new();
        write!(&mut header_str, "{link}").expect("Display impl should not fail");

        assert_eq!(
            header_str,
            r#"<http://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#,
        );
    }

    #[test]
    fn display_multiple_values() {
        let link = Link::new(vec![
            LinkValue::new("/next").push_rel(RelationType::Next),
            LinkValue::new("/prev").push_rel(RelationType::Prev),
        ]);

        let s = link.to_string();
        assert!(s.contains("</next>; rel=\"next\""));
        assert!(s.contains("</prev>; rel=\"prev\""));
        assert!(s.contains(", "));
    }

    #[test]
    fn parse_errors_missing_angle_brackets() {
        let raw = r#"http://example.com/; rel="next""#;
        assert!(raw.parse::<Link>().is_err());
    }

    #[test]
    fn parse_errors_missing_param_name() {
        let raw = r#"<http://example.com/>; ="next""#;
        assert!(raw.parse::<Link>().is_err());
    }

    #[test]
    fn parse_errors_empty_rel_value() {
        let raw = r#"<http://example.com/>; rel="#;
        assert!(raw.parse::<Link>().is_err());
    }

    #[test]
    fn split_ascii_unquoted_iterator() {
        let string = "some, text; \"and, more; in quotes\", or not";
        let mut split = SplitAsciiUnquoted::new(string, ";,");

        assert_eq!(Some("some"), split.next());
        assert_eq!(Some(" text"), split.next());
        assert_eq!(Some(" \"and, more; in quotes\""), split.next());
        assert_eq!(Some(" or not"), split.next());
        assert_eq!(None, split.next());
    }

    #[test]
    fn verify_and_trim_valid() {
        let result = verify_and_trim(">  some string   <", b'>', b'<');
        assert_eq!(result.ok(), Some("some string"));
    }

    #[test]
    fn verify_and_trim_invalid_start() {
        let result = verify_and_trim(" >  some string   <", b'>', b'<');
        assert!(result.is_err());
    }

    // -- NGSI-LD associated method tests --

    #[test]
    fn jsonld_context_link_encodes() {
        let link = Link::jsonld_context("https://example.com/context.jsonld");
        let raw = link.to_header_string().expect("should encode");
        assert!(raw.contains("<https://example.com/context.jsonld>"));
        assert!(raw.contains("rel=\"http://www.w3.org/ns/json-ld#context\""));
        assert!(raw.contains("type=\"application/ld+json\""));
    }

    #[test]
    fn multi_value_link_encodes() {
        let values = vec![
            LinkValue::with_params("https://example.com/one", Some("prev"), None),
            LinkValue::with_params("https://example.com/two", Some("next"), None),
        ];
        let link = Link::from_values(values).expect("non-empty vec should produce Some");
        let raw = link.to_header_string().expect("should encode");
        assert!(raw.contains("<https://example.com/one>"));
        assert!(raw.contains("<https://example.com/two>"));
    }

    #[test]
    fn empty_values_return_none() {
        assert!(Link::from_values(Vec::new()).is_none());
    }

    #[test]
    fn header_trait_roundtrip() {
        let link = Link::jsonld_context("https://example.com/ctx.jsonld");

        // Encode
        let mut values = Vec::new();
        link.encode(&mut values);
        assert_eq!(values.len(), 1);

        // Decode
        let mut iter = values.iter();
        let decoded = Link::decode(&mut iter).expect("should decode");
        assert_eq!(decoded.values().len(), 1);
        assert_eq!(decoded.values()[0].link(), "https://example.com/ctx.jsonld");
    }

    #[test]
    fn pagination_link_with_type() {
        let lv = LinkValue::with_params("/entities?offset=20&limit=10", Some("next"), Some("application/json"));
        let link = Link::new(vec![lv]);
        let raw = link.to_header_string().expect("should encode");
        assert!(raw.contains("rel=\"next\""));
        assert!(raw.contains("type=\"application/json\""));
    }

    // -- RelationType tests --

    #[test]
    fn relation_type_jsonld_context_constructor() {
        let rel = RelationType::jsonld_context();
        assert_eq!(rel, RelationType::ExtRelType("http://www.w3.org/ns/json-ld#context".to_owned()));
    }

    #[test]
    fn relation_type_is_jsonld_context_positive() {
        assert!(RelationType::jsonld_context().is_jsonld_context());
    }

    #[test]
    fn relation_type_is_jsonld_context_negative() {
        assert!(!RelationType::Next.is_jsonld_context());
    }

    // -- LinkValue query method tests --

    #[test]
    fn link_value_has_rel_positive() {
        let lv = LinkValue::new("/page").push_rel(RelationType::Next);
        assert!(lv.has_rel(&RelationType::Next));
    }

    #[test]
    fn link_value_has_rel_negative() {
        let lv = LinkValue::new("/page").push_rel(RelationType::Next);
        assert!(!lv.has_rel(&RelationType::Prev));
    }

    #[test]
    fn link_value_is_context_link() {
        let context_lv = LinkValue::new("https://example.com/ctx.jsonld").push_rel(RelationType::jsonld_context());
        assert!(context_lv.is_context_link());

        let pagination_lv = LinkValue::new("/page/2").push_rel(RelationType::Next);
        assert!(!pagination_lv.is_context_link());
    }

    // -- Link query method tests --

    #[test]
    fn link_context_urls_filters_correctly() {
        let link = Link::new(vec![
            LinkValue::new("https://example.com/ctx.jsonld").push_rel(RelationType::jsonld_context()),
            LinkValue::new("/page/2").push_rel(RelationType::Next),
            LinkValue::new("https://example.com/other.jsonld").push_rel(RelationType::jsonld_context()),
        ]);

        let urls: Vec<&str> = link.context_urls().collect();
        assert_eq!(urls, vec!["https://example.com/ctx.jsonld", "https://example.com/other.jsonld"]);
    }

    #[test]
    fn link_find_by_rel_returns_matching() {
        let link = Link::new(vec![
            LinkValue::new("/page/2").push_rel(RelationType::Next),
            LinkValue::new("https://example.com/ctx.jsonld").push_rel(RelationType::jsonld_context()),
            LinkValue::new("/page/3").push_rel(RelationType::Next),
        ]);

        let next_values: Vec<&str> = link.find_by_rel(RelationType::Next).map(|lv| lv.link()).collect();
        assert_eq!(next_values, vec!["/page/2", "/page/3"]);
    }

    // -- First/Last RelationType tests --

    #[test]
    fn relation_type_first_displays_correctly() {
        assert_eq!(RelationType::First.to_string(), "first");
    }

    #[test]
    fn relation_type_last_displays_correctly() {
        assert_eq!(RelationType::Last.to_string(), "last");
    }

    #[test]
    fn relation_type_first_parses_case_insensitive() {
        assert_eq!("first".parse::<RelationType>(), Ok(RelationType::First));
        assert_eq!("FIRST".parse::<RelationType>(), Ok(RelationType::First));
        assert_eq!("First".parse::<RelationType>(), Ok(RelationType::First));
    }

    #[test]
    fn relation_type_last_parses_case_insensitive() {
        assert_eq!("last".parse::<RelationType>(), Ok(RelationType::Last));
        assert_eq!("LAST".parse::<RelationType>(), Ok(RelationType::Last));
        assert_eq!("Last".parse::<RelationType>(), Ok(RelationType::Last));
    }

    #[test]
    fn parse_link_with_first_and_last_rels() {
        let raw = r#"</page?offset=0&limit=10>; rel="first", </page?offset=40&limit=10>; rel="last""#;
        let parsed: Link = raw.parse().expect("should parse");
        assert_eq!(parsed.values().len(), 2);
        assert!(parsed.values()[0].rel().unwrap().contains(&RelationType::First));
        assert!(parsed.values()[1].rel().unwrap().contains(&RelationType::Last));
    }
}
