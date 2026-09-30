//! Recognising a reference to one of the broker's own stored `@context`s.
//!
//! A request, a stored subscription or a context document may name a Hosted or
//! `ImplicitlyCreated` `@context` by the URL the broker minted for it
//! ([`crate::hosted_context_url`]), by the same URL built on another origin
//! (the request origin when no `apiRoot` is configured, a proxy's, the
//! loopback a background job dials), or by the root-relative path of the
//! resource. What makes a reference one of the broker's own is its path and its
//! tenant, never its origin: the path is the clause 6.30 resource
//! `/ngsi-ld/v1/jsonldContexts/{contextId}`, optionally below the path prefix of
//! the configured `apiRoot` (clause 6.2), naming one `urn:ngsi-ld:JsonLdContext:`
//! identifier, and the query is empty or names the tenant in `andromedaTenant`
//! and nothing else. Such a reference is resolved from storage by its
//! identifier when the tenant holds it (clause 5.13.1 serves these entries
//! "on-demand"), never fetched over HTTP from the broker itself, and only for
//! the tenant whose storage it reads (clause 4.14): a reference naming another
//! tenant's entry is an ordinary URL.

use crate::hosted_context_url::{CONTEXTS_PATH, TENANT_PARAMETER};
use andromeda_jsonld::hosted_context_recognizer::{HostedContextRecognizer, HostedReference};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::JsonLdContextId, tenant::TenantName};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriRefBuf;
use percent_encoding::percent_decode_str;
use url::{Url, form_urlencoded};

/// A reference to one stored `@context` of one tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedContextReference {
    /// The identifier of the stored `@context`.
    identifier: JsonLdContextId,
    /// The tenant whose entry it is.
    tenant: TenantRef,
}

impl HostedContextReference {
    /// The stored `@context` `reference` names, when it is shaped like one of
    /// the broker's own: an absolute hierarchical IRI on any origin, or a
    /// root-relative reference, whose path is `/ngsi-ld/v1/jsonldContexts/<identifier>`
    /// (or the same below the path prefix of `advertised`), with no fragment,
    /// and whose query is empty or exactly one `andromedaTenant`. `None` for
    /// any reference that names no stored `@context`.
    #[must_use]
    pub fn parse(reference: &IriRefBuf, advertised: Option<&Url>) -> Option<HostedContextReference> {
        let target = ReferenceTarget::of(reference)?;
        let identifier = identifier_below(target.path, "").or_else(|| {
            advertised
                .map(|root| root.path().trim_end_matches('/'))
                .filter(|prefix| !prefix.is_empty())
                .and_then(|prefix| identifier_below(target.path, prefix))
        })?;
        let tenant = tenant_of(target.query)?;
        Some(HostedContextReference { identifier, tenant })
    }

    /// The identifier of the stored `@context`.
    #[must_use]
    pub const fn identifier(&self) -> &JsonLdContextId {
        &self.identifier
    }

    /// The tenant whose entry it is.
    #[must_use]
    pub const fn tenant(&self) -> &TenantRef {
        &self.tenant
    }
}

/// The path and query a reference addresses, whatever origin it names,
/// borrowed from the reference.
struct ReferenceTarget<'a> {
    /// The path, as written (still percent-encoded).
    path: &'a str,
    /// The query, when there is one.
    query: Option<&'a str>,
}

impl<'a> ReferenceTarget<'a> {
    /// The path and query of an absolute hierarchical IRI or of a root-relative
    /// reference; `None` for anything else -- an opaque IRI such as a bare URN,
    /// a network-path or path-relative reference -- and for a reference with a
    /// fragment, which names part of a document rather than the stored
    /// `@context` itself.
    fn of(reference: &'a IriRefBuf) -> Option<ReferenceTarget<'a>> {
        let addresses_a_resource = match (reference.scheme(), reference.authority()) {
            (Some(_), Some(_)) => true,
            (None, None) => reference.path().starts_with('/'),
            (Some(_), None) | (None, Some(_)) => false,
        };
        match (addresses_a_resource, reference.fragment()) {
            (true, None) => Some(ReferenceTarget {
                path: reference.path(),
                query: reference.query(),
            }),
            (true, Some(_)) | (false, _) => None,
        }
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

/// The tenant `query` names: the default tenant with no query, the
/// `andromedaTenant` tenant, or nothing for any other parameter, a repeated
/// one, or a value that is no tenant name.
fn tenant_of(query: Option<&str>) -> Option<TenantRef> {
    let mut named: Option<TenantName> = None;
    for (key, value) in query.into_iter().flat_map(|query| form_urlencoded::parse(query.as_bytes())) {
        if key != TENANT_PARAMETER || named.is_some() {
            return None;
        }
        match value.parse::<TenantName>() {
            Ok(tenant) => named = Some(tenant),
            Err(_not_a_tenant_name) => return None,
        }
    }
    Some(TenantRef::from_header(named))
}

/// Recognises references to one tenant's own stored `@context`s.
#[derive(Debug, Clone)]
pub struct TenantHostedContexts {
    /// The configured `apiRoot`, below whose path prefix references are
    /// recognised as well as at the root.
    advertised: Option<Url>,
    /// The tenant whose storage the engine reads.
    tenant: TenantRef,
}

impl TenantHostedContexts {
    /// The recogniser of `tenant`'s own stored `@context`s.
    #[must_use]
    pub const fn new(advertised: Option<Url>, tenant: TenantRef) -> TenantHostedContexts {
        TenantHostedContexts { advertised, tenant }
    }
}

impl HostedContextRecognizer for TenantHostedContexts {
    fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
        match HostedContextReference::parse(reference, self.advertised.as_ref()) {
            Some(parsed) if parsed.tenant == self.tenant => HostedReference::Own(parsed.identifier),
            Some(_) => HostedReference::OtherTenant,
            None => HostedReference::Unrelated,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hosted_context_reference::{HostedContextReference, TenantHostedContexts};
    use andromeda_jsonld::hosted_context_recognizer::{HostedContextRecognizer, HostedReference};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::IriRefBuf;
    use url::Url;

    const ID: &str = "urn:ngsi-ld:JsonLdContext:0b6e3b5e-6a53-5bd2-9d5c-1f7d8f3e2a10";

    fn root(text: &str) -> Url {
        Url::parse(text).expect("a URL")
    }

    fn named(name: &str) -> TenantRef {
        TenantRef::Named(name.parse().expect("a tenant name"))
    }

    fn reference(text: &str) -> IriRefBuf {
        IriRefBuf::new(text).expect("an IRI reference")
    }

    fn parsed(text: &str, advertised: Option<&Url>) -> Option<(String, TenantRef)> {
        HostedContextReference::parse(&reference(text), advertised).map(|parsed| (parsed.identifier().to_string(), parsed.tenant().clone()))
    }

    #[test]
    fn a_root_relative_path_names_a_default_tenant_entry() {
        pretty_assertions::assert_eq!(
            parsed(&format!("/ngsi-ld/v1/jsonldContexts/{ID}"), None),
            Some((ID.to_owned(), TenantRef::UnnamedDefault))
        );
    }

    #[test]
    fn a_tenant_parameter_names_the_tenant() {
        pretty_assertions::assert_eq!(
            parsed(&format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme"), None),
            Some((ID.to_owned(), named("acme")))
        );
    }

    /// The origin does not decide: the broker may be named by its configured
    /// `apiRoot`, by the origin a request reached it on, or by the loopback a
    /// background job dials.
    #[test]
    fn an_absolute_url_on_any_origin_names_a_stored_entry() {
        for origin in ["http://127.0.0.1:1056", "https://gateway.example.com", "http://[::1]:8080"] {
            pretty_assertions::assert_eq!(
                parsed(&format!("{origin}/ngsi-ld/v1/jsonldContexts/{ID}"), None),
                Some((ID.to_owned(), TenantRef::UnnamedDefault)),
                "{origin}"
            );
        }
    }

    /// Clause 6.2: the `apiRoot` may carry a path prefix, below which the
    /// resource sits; the unprefixed path is still recognised.
    #[test]
    fn the_configured_api_root_path_prefix_is_recognised() {
        let advertised = root("https://proxy.example/broker/");
        pretty_assertions::assert_eq!(
            parsed(&format!("https://any.example/broker/ngsi-ld/v1/jsonldContexts/{ID}"), Some(&advertised)),
            Some((ID.to_owned(), TenantRef::UnnamedDefault))
        );
        pretty_assertions::assert_eq!(
            parsed(&format!("/broker/ngsi-ld/v1/jsonldContexts/{ID}"), Some(&advertised)),
            Some((ID.to_owned(), TenantRef::UnnamedDefault))
        );
        pretty_assertions::assert_eq!(
            parsed(&format!("https://any.example/ngsi-ld/v1/jsonldContexts/{ID}"), Some(&advertised)),
            Some((ID.to_owned(), TenantRef::UnnamedDefault))
        );
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
            Some((ID.to_owned(), TenantRef::UnnamedDefault))
        );
        pretty_assertions::assert_eq!(
            parsed(&format!("/ngsi-ld/v1/jsonldContexts/{encoded}?andromedaTenant=acme"), None),
            Some((ID.to_owned(), named("acme")))
        );
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
            format!("/ngsi-ld/v1/jsonldContexts/{ID}?details=true"),
            format!("http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{ID}?details=true"),
            format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=a&andromedaTenant=b"),
            format!("/ngsi-ld/v1/jsonldContexts/{ID}#fragment"),
            format!("ngsi-ld/v1/jsonldContexts/{ID}"),
            format!("//other.example/ngsi-ld/v1/jsonldContexts/{ID}"),
            ID.to_owned(),
        ] {
            pretty_assertions::assert_eq!(parsed(&reference, None), None, "{reference}");
        }
    }

    /// Clause 4.14: only the tenant's own entries resolve from its storage; an
    /// entry of another tenant is recognised as such, and anything else is
    /// unrelated.
    #[test]
    fn a_tenant_recognises_its_own_entries_and_tells_another_tenants_apart() {
        let acme = TenantHostedContexts::new(None, named("acme"));
        let own = HostedReference::Own(ID.parse().expect("an identifier"));

        pretty_assertions::assert_eq!(
            acme.recognize(&reference(&format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme"))),
            own
        );
        pretty_assertions::assert_eq!(
            acme.recognize(&reference(&format!(
                "http://127.0.0.1:1056/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme"
            ))),
            own
        );
        pretty_assertions::assert_eq!(
            acme.recognize(&reference(&format!("/ngsi-ld/v1/jsonldContexts/{ID}"))),
            HostedReference::OtherTenant
        );
        pretty_assertions::assert_eq!(
            acme.recognize(&reference(&format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=other"))),
            HostedReference::OtherTenant
        );
        pretty_assertions::assert_eq!(acme.recognize(&reference("https://example.com/context.jsonld")), HostedReference::Unrelated);

        let default = TenantHostedContexts::new(None, TenantRef::UnnamedDefault);
        pretty_assertions::assert_eq!(
            default.recognize(&reference(&format!("https://gateway.example.com/ngsi-ld/v1/jsonldContexts/{ID}"))),
            own
        );
        pretty_assertions::assert_eq!(
            default.recognize(&reference(&format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=acme"))),
            HostedReference::OtherTenant
        );
    }

    /// A tenant parameter whose value is no tenant name names no stored entry.
    #[test]
    fn a_tenant_parameter_that_is_no_tenant_name_names_no_stored_entry() {
        pretty_assertions::assert_eq!(parsed(&format!("/ngsi-ld/v1/jsonldContexts/{ID}?andromedaTenant=%20"), None), None);
    }
}
