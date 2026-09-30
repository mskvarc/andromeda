//! Finding the JSON-LD alternate a non-JSON response names.
//!
//! JSON-LD 1.1 section 6.2 lets "a non-JSON resource (i.e., one using a media
//! type other than application/json or a derivative) [...] return the
//! alternate location using a Link Header with: rel="alternate", and
//! type="application/ld+json"", and the JSON-LD 1.1 API's `LoadDocumentCallback`
//! follows it. The specification's own Annex C.8 names `https://schema.org` as
//! a context, and schema.org answers a JSON-LD `Accept` with an HTML page that
//! carries exactly such a link, so without following it the example does not
//! resolve.

use andromeda_http_headers::link::{error::LinkParseError, header::Link, relation_type::RelationType};
use iri_rs::{InvalidIri, IriBuf, IriParseError, IriRefBuf};
use mediatype::MediaTypeBuf;
use reqwest::header::{HeaderMap, HeaderValue, LINK, ToStrError};
use std::str::FromStr;
use thiserror::Error;
use tracing::warn;

/// What a refused response offers instead of itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonLdAlternate {
    /// It names no JSON-LD alternate.
    None,
    /// It names exactly one, resolved against the response's own URL.
    One(IriBuf),
    /// It names this many, so which one is the context is not stated.
    Ambiguous(usize),
}

/// Why a `Link` field of a refused response offers no usable alternate.
#[derive(Debug, Error)]
enum UnusableLink {
    /// The field holds bytes outside visible ASCII.
    #[error("the Link field is not visible ASCII")]
    NotVisibleAscii(#[source] ToStrError),
    /// The field is not an IETF RFC 8288 `Link` header.
    #[error("the Link field does not parse")]
    Malformed(#[source] LinkParseError),
    /// A JSON-LD alternate's target does not resolve against the response URL.
    #[error("the alternate target does not resolve against the response URL")]
    UnresolvableTarget(#[source] IriParseError),
    /// A JSON-LD alternate's resolved target is not an absolute IRI.
    #[error("the resolved alternate target is not an absolute IRI")]
    TargetNotIri(#[source] InvalidIri<String>),
}

/// The JSON-LD alternate the `Link` headers of a response read from
/// `document_url` name.
///
/// A field that does not parse, and an alternate whose target does not
/// resolve against `document_url`, offer no usable alternate: the response is
/// being refused already, and a link nobody can read cannot rescue it. The
/// reason is logged, and the other fields are still read.
pub(crate) fn json_ld_alternate(headers: &HeaderMap, document_url: &IriBuf) -> JsonLdAlternate {
    let alternates: Vec<IriBuf> = headers
        .get_all(LINK)
        .iter()
        .flat_map(|field| match parsed_link(field) {
            Ok(link) => json_ld_targets(&link, document_url),
            Err(unusable) => vec![Err(unusable)],
        })
        .filter_map(|target| match target {
            Ok(alternate) => Some(alternate),
            Err(unusable) => {
                warn!(url = %document_url, reason = %unusable, "A Link field of a refused context response offers no usable alternate");
                None
            }
        })
        .collect();
    match <[IriBuf; 1]>::try_from(alternates) {
        Ok([alternate]) => JsonLdAlternate::One(alternate),
        Err(several) if several.is_empty() => JsonLdAlternate::None,
        Err(several) => JsonLdAlternate::Ambiguous(several.len()),
    }
}

/// One `Link` field, parsed.
fn parsed_link(field: &HeaderValue) -> Result<Link, UnusableLink> {
    let text = field.to_str().map_err(UnusableLink::NotVisibleAscii)?;
    Link::from_str(text).map_err(UnusableLink::Malformed)
}

/// The targets of the JSON-LD alternates one `Link` field names, each resolved
/// against `document_url`.
///
/// The `alternate` relation is recognised whatever its case (RFC 8288 section
/// 2.1.1), by [`RelationType::Alternate`].
fn json_ld_targets(link: &Link, document_url: &IriBuf) -> Vec<Result<IriBuf, UnusableLink>> {
    link.values()
        .iter()
        .filter(|value| value.has_rel(&RelationType::Alternate) && value.media_type().is_some_and(is_json_ld))
        .map(|value| resolved_target(value.link(), document_url))
        .collect()
}

/// An alternate's target resolved against `document_url`.
fn resolved_target(target: &IriRefBuf, document_url: &IriBuf) -> Result<IriBuf, UnusableLink> {
    let resolved = target.resolved(document_url).map_err(UnusableLink::UnresolvableTarget)?;
    IriBuf::new(resolved.into_inner()).map_err(UnusableLink::TargetNotIri)
}

/// Whether a `type` parameter names `application/ld+json`.
fn is_json_ld(media_type: &MediaTypeBuf) -> bool {
    media_type.ty().as_str().eq_ignore_ascii_case("application")
        && media_type.subty().as_str().eq_ignore_ascii_case("ld")
        && media_type.suffix().is_some_and(|suffix| suffix.as_str().eq_ignore_ascii_case("json"))
}

#[cfg(test)]
mod tests {
    use crate::loader::alternate_link::{JsonLdAlternate, UnusableLink, json_ld_alternate, parsed_link};
    use iri_rs::IriBuf;
    use reqwest::header::{HeaderMap, HeaderValue, LINK};

    fn base() -> IriBuf {
        IriBuf::new("https://schema.example/".to_owned()).expect("a valid IRI")
    }

    fn headers(fields: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for field in fields {
            headers.append(LINK, HeaderValue::from_str(field).expect("a valid header value"));
        }
        headers
    }

    /// The shape schema.org answers with: one alternate, a relative target
    /// resolved against the response's URL.
    #[test]
    fn one_json_ld_alternate_is_found_and_resolved() {
        let found = json_ld_alternate(
            &headers(&[r#"</docs/jsonldcontext.jsonld>; rel="alternate"; type="application/ld+json""#]),
            &base(),
        );

        pretty_assertions::assert_eq!(
            found,
            JsonLdAlternate::One(IriBuf::new("https://schema.example/docs/jsonldcontext.jsonld".to_owned()).expect("a valid IRI"))
        );
    }

    #[test]
    fn two_json_ld_alternates_are_ambiguous() {
        let found = json_ld_alternate(
            &headers(&[
                r#"</a.jsonld>; rel="alternate"; type="application/ld+json""#,
                r#"</b.jsonld>; rel="alternate"; type="application/ld+json""#,
            ]),
            &base(),
        );

        pretty_assertions::assert_eq!(found, JsonLdAlternate::Ambiguous(2));
    }

    /// An alternate of another media type, a non-alternate relation and a
    /// field that does not parse name no JSON-LD alternate.
    #[test]
    fn other_links_name_no_json_ld_alternate() {
        let found = json_ld_alternate(
            &headers(&[r#"</page.html>; rel="alternate"; type="text/html""#, r#"</next>; rel="next""#, "not a link"]),
            &base(),
        );

        pretty_assertions::assert_eq!(found, JsonLdAlternate::None);
    }

    /// A malformed or non-ASCII field offers no usable alternate, with a typed
    /// reason, and does not hide the alternate another field names.
    #[test]
    fn a_malformed_link_field_offers_no_alternate_and_leaves_the_others_readable() {
        let malformed = parsed_link(&HeaderValue::from_static("<unterminated; rel=alternate")).expect_err("the field does not parse");
        assert!(matches!(malformed, UnusableLink::Malformed(_)), "{malformed:?}");
        let non_ascii = parsed_link(&HeaderValue::from_bytes(b"</c.jsonld>; title=\"caf\xe9\"").expect("an opaque header value"))
            .expect_err("the field is not visible ASCII");
        assert!(matches!(non_ascii, UnusableLink::NotVisibleAscii(_)), "{non_ascii:?}");

        let mut fields = headers(&["<unterminated; rel=alternate", r#"</c.jsonld>; rel="alternate"; type="application/ld+json""#]);
        fields.append(
            LINK,
            HeaderValue::from_bytes(b"</d.jsonld>; title=\"caf\xe9\"").expect("an opaque header value"),
        );

        pretty_assertions::assert_eq!(
            json_ld_alternate(&fields, &base()),
            JsonLdAlternate::One(IriBuf::new("https://schema.example/c.jsonld".to_owned()).expect("a valid IRI"))
        );
    }

    #[test]
    fn the_relation_name_is_matched_case_insensitively() {
        let found = json_ld_alternate(&headers(&[r#"</c.jsonld>; rel="Alternate"; type="application/ld+json""#]), &base());

        assert!(matches!(found, JsonLdAlternate::One(_)), "{found:?}");
    }
}
