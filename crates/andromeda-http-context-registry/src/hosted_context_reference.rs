//! Recognising a reference to one of the broker's own stored `@context`s.
//!
//! A request, a stored subscription or a context document may name a Hosted or
//! `ImplicitlyCreated` `@context` by the URL the broker minted for it
//! ([`crate::hosted_context_url`]), by the same URL built on another origin
//! (the request origin when no `apiRoot` is configured, a proxy's, the
//! loopback a background job dials), or by the root-relative path of the
//! resource. What makes a reference one of the broker's own is its path, never
//! its origin: the path is the clause 6.30 resource
//! `/ngsi-ld/v1/jsonldContexts/{contextId}`, optionally below the path prefix of
//! the configured `apiRoot` (clause 6.2), naming one `urn:ngsi-ld:JsonLdContext:`
//! identifier, with no query and no fragment. Such a reference is resolved from
//! storage by its identifier when the tenant holds it (clause 5.13.1 serves
//! these entries "on-demand"), never fetched over HTTP from the broker itself.
//!
//! A reference names no tenant. Clauses 4.14 and 6.3.14 make the
//! `NGSILD-Tenant` header the only way a request names a tenant, so like every
//! other resource path the reference names an entry of the tenant whose storage
//! reads it, whether it is absolute or root-relative.

use crate::hosted_context_url::CONTEXTS_PATH;
use andromeda_jsonld::hosted_context_recognizer::{HostedContextRecognizer, HostedReference};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
use iri_rs::IriRefBuf;
use percent_encoding::percent_decode_str;
use url::Url;

/// A reference to one stored `@context`, of whichever tenant reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedContextReference {
    /// The identifier of the stored `@context`.
    identifier: JsonLdContextId,
}

impl HostedContextReference {
    /// The stored `@context` `reference` names, when it is shaped like one of
    /// the broker's own: an absolute hierarchical IRI on any origin, or a
    /// root-relative reference, whose path is `/ngsi-ld/v1/jsonldContexts/<identifier>`
    /// (or the same below the path prefix of `advertised`), with no query and
    /// no fragment. `None` for any reference that names no stored `@context`.
    #[must_use]
    pub fn parse(reference: &IriRefBuf, advertised: Option<&Url>) -> Option<HostedContextReference> {
        let path = addressed_path(reference)?;
        let identifier = identifier_below(path, "").or_else(|| {
            advertised
                .map(|root| root.path().trim_end_matches('/'))
                .filter(|prefix| !prefix.is_empty())
                .and_then(|prefix| identifier_below(path, prefix))
        })?;
        Some(HostedContextReference { identifier })
    }

    /// The identifier of the stored `@context`.
    #[must_use]
    pub const fn identifier(&self) -> &JsonLdContextId {
        &self.identifier
    }
}

/// The path, as written (still percent-encoded), of an absolute hierarchical
/// IRI on whatever origin or of a root-relative reference; `None` for anything
/// else -- an opaque IRI such as a bare URN, a network-path or path-relative
/// reference -- and for a reference with a query or a fragment. A query is no
/// part of any URL the broker mints for a stored `@context`, and a fragment
/// names part of a document rather than the stored `@context` itself.
fn addressed_path(reference: &IriRefBuf) -> Option<&str> {
    let shaped = match (reference.scheme(), reference.authority()) {
        (Some(_), Some(_)) => true,
        (None, None) => reference.path().starts_with('/'),
        (Some(_), None) | (None, Some(_)) => false,
    };
    let path: &str = reference.path();
    match (shaped, reference.query(), reference.fragment()) {
        (true, None, None) => Some(path),
        (true, Some(_), _) | (true, None, Some(_)) | (false, _, _) => None,
    }
}

/// The identifier `path` names below the `apiRoot` path `prefix`: exactly the
/// clause 6.30 path followed by one segment, percent-decoded, that parses as a
/// `JsonLdContextId`.
fn identifier_below(path: &str, prefix: &str) -> Option<JsonLdContextId> {
    let below = path.strip_prefix(prefix)?;
    let mut segments = below.strip_prefix('/')?.split('/');
    let path_matches = CONTEXTS_PATH.iter().all(|expected| segments.next() == Some(expected));
    match (path_matches, segments.next(), segments.next()) {
        (true, Some(segment), None) => stored_identifier(segment),
        (true, Some(_), Some(_)) | (true, None, _) | (false, _, _) => None,
    }
}

/// The identifier one path segment names, decoded as a URL path segment is;
/// `None` when the segment names no `JsonLdContext` identifier.
fn stored_identifier(segment: &str) -> Option<JsonLdContextId> {
    match percent_decode_str(segment).decode_utf8().map(|decoded| decoded.parse::<JsonLdContextId>()) {
        Ok(Ok(identifier)) => Some(identifier),
        // A segment that is not UTF-8, or names no `JsonLdContext`
        // identifier, names no stored `@context`.
        Ok(Err(_)) | Err(_) => None,
    }
}

/// Recognises references to the stored `@context`s of the tenant whose
/// storage the engine reads.
#[derive(Debug, Clone)]
pub struct TenantHostedContexts {
    /// The configured `apiRoot`, below whose path prefix references are
    /// recognised as well as at the root.
    advertised: Option<Url>,
}

impl TenantHostedContexts {
    /// The recogniser of a tenant's own stored `@context`s, below the
    /// configured `advertised` `apiRoot` as well as at the root.
    #[must_use]
    pub const fn new(advertised: Option<Url>) -> TenantHostedContexts {
        TenantHostedContexts { advertised }
    }
}

impl HostedContextRecognizer for TenantHostedContexts {
    fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
        HostedContextReference::parse(reference, self.advertised.as_ref()).map_or(HostedReference::Unrelated, |parsed| HostedReference::Own(parsed.identifier))
    }
}

#[cfg(test)]
mod tests {
    use crate::hosted_context_reference::{HostedContextReference, TenantHostedContexts};
    use andromeda_jsonld::hosted_context_recognizer::{HostedContextRecognizer, HostedReference};
    use iri_rs::IriRefBuf;
    use url::Url;

    const ID: &str = "urn:ngsi-ld:JsonLdContext:0b6e3b5e-6a53-5bd2-9d5c-1f7d8f3e2a10";

    fn root(text: &str) -> Url {
        Url::parse(text).expect("a URL")
    }

    fn reference(text: &str) -> IriRefBuf {
        IriRefBuf::new(text).expect("an IRI reference")
    }

    fn parsed(text: &str, advertised: Option<&Url>) -> Option<String> {
        HostedContextReference::parse(&reference(text), advertised).map(|parsed| parsed.identifier().to_string())
    }

    /// Clauses 6.29.3.1 and 4.14: the root-relative `Location` Add @context
    /// answers names the entry by its identifier.
    #[test]
    fn a_root_relative_path_names_a_stored_entry() {
        pretty_assertions::assert_eq!(parsed(&format!("/ngsi-ld/v1/jsonldContexts/{ID}"), None), Some(ID.to_owned()));
    }

    /// The origin does not decide: the broker may be named by its configured
    /// `apiRoot`, by the origin a request reached it on, or by the loopback a
    /// background job dials.
    #[test]
    fn an_absolute_url_on_any_origin_names_a_stored_entry() {
        for origin in ["http://127.0.0.1:1056", "https://gateway.example.com", "http://[::1]:8080"] {
            pretty_assertions::assert_eq!(
                parsed(&format!("{origin}/ngsi-ld/v1/jsonldContexts/{ID}"), None),
                Some(ID.to_owned()),
                "{origin}"
            );
        }
    }

    /// Clause 6.2: the `apiRoot` may carry a path prefix, below which the
    /// resource sits; the unprefixed path is still recognised.
    #[test]
    fn the_configured_api_root_path_prefix_is_recognised() {
        let advertised = root("https://proxy.example/broker/");
        for reference in [
            format!("https://any.example/broker/ngsi-ld/v1/jsonldContexts/{ID}"),
            format!("/broker/ngsi-ld/v1/jsonldContexts/{ID}"),
            format!("https://any.example/ngsi-ld/v1/jsonldContexts/{ID}"),
        ] {
            pretty_assertions::assert_eq!(parsed(&reference, Some(&advertised)), Some(ID.to_owned()), "{reference}");
        }
    }

    /// Only the configured prefix counts: another path in front of the
    /// resource is someone else's.
    #[test]
    fn a_path_prefix_that_is_not_the_configured_one_is_an_ordinary_reference() {
        let advertised = root("https://proxy.example/broker");
        pretty_assertions::assert_eq!(
            parsed(&format!("https://any.example/other/ngsi-ld/v1/jsonldContexts/{ID}"), Some(&advertised)),
            None
        );
        pretty_assertions::assert_eq!(parsed(&format!("https://any.example/broker/ngsi-ld/v1/jsonldContexts/{ID}"), None), None);
    }

    /// A client may percent-encode the colons of the identifier segment; the
    /// segment is decoded as a URL path segment is.
    #[test]
    fn a_percent_encoded_identifier_names_the_same_entry() {
        let encoded = ID.replace(':', "%3A");
        pretty_assertions::assert_eq!(
            parsed(&format!("http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{encoded}"), None),
            Some(ID.to_owned())
        );
        pretty_assertions::assert_eq!(parsed(&format!("/ngsi-ld/v1/jsonldContexts/{encoded}"), None), Some(ID.to_owned()));
    }

    /// No URL the broker mints for a stored `@context` carries a query, so a
    /// reference with any query, the former tenant parameter included, is not
    /// one of the broker's own.
    #[test]
    fn a_reference_with_any_query_is_not_ours() {
        for query in ["?andromedaTenant=acme", "?x=1", "?details=true", "?"] {
            for origin in ["", "http://127.0.0.1:1056"] {
                let reference = format!("{origin}/ngsi-ld/v1/jsonldContexts/{ID}{query}");
                pretty_assertions::assert_eq!(parsed(&reference, None), None, "{reference}");
            }
        }
    }

    #[test]
    fn anything_else_is_an_ordinary_reference() {
        for reference in [
            "https://example.com/context.jsonld".to_owned(),
            "/ngsi-ld/v1/jsonldContexts/not-an-identifier".to_owned(),
            "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Subscription:0b6e3b5e-6a53-5bd2-9d5c-1f7d8f3e2a10".to_owned(),
            "/ngsi-ld/v1/jsonldContexts/".to_owned(),
            format!("/ngsi-ld/v1/jsonldContexts/{ID}/extra"),
            format!("http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{ID}/extra"),
            format!("/ngsi-ld/v1/jsonldContexts/{ID}#fragment"),
            format!("ngsi-ld/v1/jsonldContexts/{ID}"),
            format!("//other.example/ngsi-ld/v1/jsonldContexts/{ID}"),
            ID.to_owned(),
        ] {
            pretty_assertions::assert_eq!(parsed(&reference, None), None, "{reference}");
        }
    }

    /// Clauses 4.14 and 6.3.14: a reference names no tenant, so an absolute
    /// reference on any origin, like a root-relative one, names an entry of the
    /// tenant whose storage reads it, default and named tenants alike.
    #[test]
    fn an_absolute_reference_names_the_reading_tenant() {
        let recognizer = TenantHostedContexts::new(None);
        let own = HostedReference::Own(ID.parse().expect("an identifier"));

        for text in [
            format!("/ngsi-ld/v1/jsonldContexts/{ID}"),
            format!("http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{ID}"),
            format!("https://gateway.example.com/ngsi-ld/v1/jsonldContexts/{ID}"),
        ] {
            pretty_assertions::assert_eq!(recognizer.recognize(&reference(&text)), own, "{text}");
        }
        pretty_assertions::assert_eq!(
            recognizer.recognize(&reference(&format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme"))),
            HostedReference::Unrelated
        );
        pretty_assertions::assert_eq!(
            recognizer.recognize(&reference("https://example.com/context.jsonld")),
            HostedReference::Unrelated
        );
    }
}
