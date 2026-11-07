// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! The `Link` header itself: a list of link-values, and how it is spelled.
//!
//! This file is derived from the disabled `Link` implementation in
//! <https://github.com/hyperium/headers> and includes local modifications.

use crate::link::{
    error::LinkParseError,
    quoting::{SplitAsciiUnquoted, verify_and_trim},
    relation_type::RelationType,
    value::LinkValue,
};
use headers::HeaderMapExt;
use http::{HeaderMap, HeaderName, HeaderValue};
use iri_rs::{IriBuf, IriRefBuf};
use std::{fmt, iter, str::FromStr};

/// Typed `Link` header (RFC 8288).
///
/// Holds one or more [`LinkValue`]s, each a single `link-value` of the header.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Link {
    /// The link-values, in the order they were written.
    values: Vec<LinkValue>,
}

impl Link {
    /// A `Link` header over the given link-values.
    #[must_use]
    pub const fn new(link_values: Vec<LinkValue>) -> Link {
        Link { values: link_values }
    }

    /// The link-values this header carries.
    #[must_use]
    pub fn values(&self) -> &[LinkValue] {
        &self.values
    }

    /// A `Link` header over the given link-values, or `None` if there are none.
    ///
    /// A `Link` header with no link-value is not a valid header, so the empty
    /// case is answered with "send nothing" rather than with an empty header.
    #[must_use]
    pub fn from_values(values: Vec<LinkValue>) -> Option<Link> {
        if values.is_empty() { None } else { Some(Link::new(values)) }
    }

    /// A single-value JSON-LD context `Link` header naming an absolute context.
    #[must_use]
    pub fn jsonld_context(context_url: IriBuf) -> Link {
        Link::new(vec![LinkValue::jsonld_context(context_url)])
    }

    /// Insert the one JSON-LD context link a response carries.
    ///
    /// JSON-LD 1.1 section 6.1: "A response MUST NOT contain more than one
    /// HTTP Link Header using the <http://www.w3.org/ns/json-ld#context> link
    /// relation", which the JSON-LD 1.1 API enforces as `multiple context link
    /// headers`. Taking a single target makes a second context link
    /// unrepresentable here; a compound context is named through one hosted
    /// wrapper instead (ETSI GS CIM 009 v1.9.1 Annex C.8).
    pub fn insert_jsonld_context(headers: &mut HeaderMap, url: IriBuf) {
        headers.typed_insert(Link::jsonld_context(url));
    }

    /// Encode this header into a raw [`HeaderValue`], if it encodes at all.
    #[must_use]
    pub fn to_header_value(&self) -> Option<HeaderValue> {
        HeaderValue::from_str(&self.to_string()).ok()
    }

    /// Encode this header into a string, if it encodes at all.
    #[must_use]
    pub fn to_header_string(&self) -> Option<String> {
        self.to_header_value().and_then(|value| value.to_str().ok().map(ToOwned::to_owned))
    }

    /// The target IRIs of every JSON-LD context link-value.
    pub fn context_urls(&self) -> impl Iterator<Item = &IriRefBuf> {
        self.values().iter().filter(|value| value.is_context_link()).map(LinkValue::link)
    }

    /// The link-values carrying a given relation type.
    pub fn find_by_rel(&self, rel: RelationType) -> impl Iterator<Item = &LinkValue> {
        self.values().iter().filter(move |value| value.has_rel(&rel))
    }
}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, value) in self.values.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{value}")?;
        }
        Ok(())
    }
}

impl FromStr for Link {
    type Err = LinkParseError;

    /// Parse a header value into link-values.
    ///
    /// Segmentation is this type's job and parameter grammar is
    /// [`LinkValue`]'s: a segment opening with `<` starts a new link-value, and
    /// every following segment is a parameter of the one before it.
    fn from_str(raw: &str) -> Result<Link, LinkParseError> {
        let mut values: Vec<LinkValue> = Vec::new();

        for segment in SplitAsciiUnquoted::new(raw, ";,") {
            let trimmed = segment.trim();
            if trimmed.starts_with('<') {
                let target = verify_and_trim(trimmed, b'<', b'>')?.parse::<IriRefBuf>().map_err(|_| LinkParseError)?;
                values.push(LinkValue::new(target));
            } else {
                values.last_mut().ok_or(LinkParseError)?.apply_param(segment)?;
            }
        }

        Ok(Link::new(values))
    }
}

/// The header name, as a `HeaderName` the `headers` trait can hand back.
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
            let parsed: Link = value
                .to_str()
                .map_err(|_| headers::Error::invalid())?
                .parse()
                .map_err(|_| headers::Error::invalid())?;
            match combined {
                None => combined = Some(parsed),
                Some(ref mut existing) => existing.values.extend(parsed.values),
            }
        }
        combined.ok_or_else(headers::Error::invalid)
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Some(value) = self.to_header_value() {
            values.extend(iter::once(value));
        } else {
            tracing::warn!(link = %self, "failed to encode Link header");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::link::{header::Link, relation_type::RelationType, value::LinkValue};
    use headers::Header;
    use http::{HeaderMap, HeaderValue};
    use iri_rs::{IriBuf, IriRefBuf};
    use mediatype::MediaTypeBuf;
    use std::iter;

    /// A validated IRI reference, for building link targets in tests.
    fn iri_ref(value: &str) -> IriRefBuf {
        IriRefBuf::new(value).expect("valid IRI reference")
    }

    /// A validated absolute IRI, for building context link targets in tests.
    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value).expect("valid IRI")
    }

    /// Decode a `Link` from a raw header string through the `headers` trait.
    fn decode_link(raw: &str) -> Option<Link> {
        let value = HeaderValue::from_str(raw).ok()?;
        let mut values = iter::once(&value);
        Link::decode(&mut values).ok()
    }

    #[test]
    fn a_single_link_value_parses_with_its_parameters() {
        let expected = Link::new(vec![
            LinkValue::new(iri_ref("http://example.com/TheBook/chapter2"))
                .push_rel(RelationType::Prev)
                .set_media_type("text/html".parse::<MediaTypeBuf>().expect("valid media type")),
        ]);

        let parsed = decode_link(r#"<http://example.com/TheBook/chapter2>; rel="prev"; type="text/html""#);
        pretty_assertions::assert_eq!(parsed, Some(expected));
    }

    #[test]
    fn comma_separated_link_values_parse_as_separate_links() {
        let expected = Link::new(vec![
            LinkValue::new(iri_ref("/page/2")).push_rel(RelationType::Next),
            LinkValue::new(iri_ref("/page/0")).push_rel(RelationType::Prev),
        ]);

        pretty_assertions::assert_eq!(decode_link(r#"</page/2>; rel="next", </page/0>; rel="prev""#), Some(expected));
    }

    #[test]
    fn a_json_ld_context_link_parses_into_its_target_and_relation() {
        let raw = r#"<http://json-ld.org/contexts/person.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;
        let parsed = decode_link(raw).expect("should parse");

        pretty_assertions::assert_eq!(parsed.values()[0].link().as_str(), "http://json-ld.org/contexts/person.jsonld");
        pretty_assertions::assert_eq!(
            parsed.values()[0].rel().expect("has rel")[0].to_string(),
            "http://www.w3.org/ns/json-ld#context"
        );
    }

    #[test]
    fn unknown_parameters_do_not_prevent_the_link_from_parsing() {
        // RFC 8288 section 3.4.1: a recipient ignores parameters it does not
        // recognise, which is what keeps other brokers' extra parameters from
        // costing a request.
        let parsed = decode_link(r#"<http://example.com/>; rel="next"; title="Chapter 2"; hreflang=en"#).expect("should parse");
        pretty_assertions::assert_eq!(parsed.values().len(), 1);
        pretty_assertions::assert_eq!(parsed.values()[0].link().as_str(), "http://example.com/");
    }

    #[test]
    fn a_link_renders_its_values_comma_separated() {
        let link = Link::new(vec![
            LinkValue::new(iri_ref("/next")).push_rel(RelationType::Next),
            LinkValue::new(iri_ref("/prev")).push_rel(RelationType::Prev),
        ]);
        pretty_assertions::assert_eq!(link.to_string(), r#"</next>; rel="next", </prev>; rel="prev""#);
    }

    #[test]
    fn a_target_that_is_not_bracketed_is_rejected() {
        assert!(r#"http://example.com/; rel="next""#.parse::<Link>().is_err());
    }

    #[test]
    fn a_malformed_parameter_is_rejected() {
        assert!(r#"<http://example.com/>; ="next""#.parse::<Link>().is_err());
        assert!(r"<http://example.com/>; rel=".parse::<Link>().is_err());
        assert!(r"<http://example.com/>; profile=".parse::<Link>().is_err());
    }

    #[test]
    fn a_header_with_no_link_value_encodes_to_nothing() {
        assert!(Link::from_values(Vec::new()).is_none());
    }

    #[test]
    fn a_json_ld_context_header_encodes_with_relation_and_media_type() {
        let raw = Link::jsonld_context(iri("https://example.com/context.jsonld"))
            .to_header_string()
            .expect("encodes");
        pretty_assertions::assert_eq!(
            raw,
            r#"<https://example.com/context.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#
        );
    }

    /// JSON-LD 1.1 section 6.1: a response names exactly one context link.
    #[test]
    fn a_response_context_is_inserted_as_exactly_one_context_link() {
        let mut headers = HeaderMap::new();
        Link::insert_jsonld_context(&mut headers, iri("https://example.com/ctx.jsonld"));

        let values: Vec<&HeaderValue> = headers.get_all("link").iter().collect();
        pretty_assertions::assert_eq!(values.len(), 1);
        let link: Link = values[0].to_str().expect("visible ASCII").parse().expect("a Link header");
        let targets: Vec<&str> = link.context_urls().map(IriRefBuf::as_str).collect();
        pretty_assertions::assert_eq!(targets, vec!["https://example.com/ctx.jsonld"]);
    }

    #[test]
    fn several_link_values_encode_into_one_header() {
        let link = Link::from_values(vec![
            LinkValue::with_params(iri_ref("https://example.com/one"), Some(RelationType::Prev), None),
            LinkValue::with_params(iri_ref("https://example.com/two"), Some(RelationType::Next), None),
        ])
        .expect("non-empty");
        let raw = link.to_header_string().expect("encodes");
        assert!(raw.contains("<https://example.com/one>"));
        assert!(raw.contains("<https://example.com/two>"));
    }

    #[test]
    fn a_pagination_link_carries_the_negotiated_media_type() {
        let link = Link::new(vec![LinkValue::with_params(
            iri_ref("/entities?offset=20&limit=10"),
            Some(RelationType::Next),
            Some("application/json".parse::<MediaTypeBuf>().expect("valid media type")),
        )]);
        pretty_assertions::assert_eq!(
            link.to_header_string().expect("encodes"),
            r#"</entities?offset=20&limit=10>; rel="next"; type="application/json""#
        );
    }

    #[test]
    fn the_encoding_decodes_back_to_the_same_header() {
        let link = Link::jsonld_context(iri("https://example.com/ctx.jsonld"));
        let mut encoded = Vec::new();
        link.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);

        let mut values = encoded.iter();
        pretty_assertions::assert_eq!(Link::decode(&mut values).expect("decodes"), link);
    }

    #[test]
    fn context_urls_returns_only_the_context_links() {
        let link = Link::new(vec![
            LinkValue::new(iri_ref("https://example.com/ctx.jsonld")).push_rel(RelationType::JsonLdContext),
            LinkValue::new(iri_ref("/page/2")).push_rel(RelationType::Next),
            LinkValue::new(iri_ref("https://example.com/other.jsonld")).push_rel(RelationType::JsonLdContext),
        ]);

        let urls: Vec<&str> = link.context_urls().map(IriRefBuf::as_str).collect();
        pretty_assertions::assert_eq!(urls, vec!["https://example.com/ctx.jsonld", "https://example.com/other.jsonld"]);
    }

    #[test]
    fn find_by_rel_returns_every_value_with_that_relation() {
        let link = Link::new(vec![
            LinkValue::new(iri_ref("/page/2")).push_rel(RelationType::Next),
            LinkValue::new(iri_ref("https://example.com/ctx.jsonld")).push_rel(RelationType::JsonLdContext),
            LinkValue::new(iri_ref("/page/3")).push_rel(RelationType::Next),
        ]);

        let targets: Vec<&str> = link.find_by_rel(RelationType::Next).map(|value| value.link().as_str()).collect();
        pretty_assertions::assert_eq!(targets, vec!["/page/2", "/page/3"]);
    }

    #[test]
    fn the_pagination_relations_parse_from_a_header() {
        let parsed: Link = r#"</page?offset=0&limit=10>; rel="first", </page?offset=40&limit=10>; rel="last""#
            .parse()
            .expect("should parse");
        pretty_assertions::assert_eq!(parsed.values().len(), 2);
        assert!(parsed.values()[0].has_rel(&RelationType::First));
        assert!(parsed.values()[1].has_rel(&RelationType::Last));
    }

    #[test]
    fn a_profile_parameter_survives_the_round_trip() {
        let raw = r#"<http://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"; profile="https://uri.etsi.org/ngsi-ld/profile/version""#;
        let parsed = decode_link(raw).expect("should parse");
        pretty_assertions::assert_eq!(
            parsed.values()[0].profile().map(IriBuf::as_str),
            Some("https://uri.etsi.org/ngsi-ld/profile/version")
        );
        pretty_assertions::assert_eq!(parsed.to_string().parse::<Link>().expect("reparses"), parsed);
    }
}
