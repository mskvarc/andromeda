//! Where one request named its `@context`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.5 gives a request exactly one place to
//! name its context: a `Link` header with the JSON-LD context relation for a
//! request without a body or with an `application/json` one, the body's own
//! `@context` member for an `application/ld+json` one, and neither -- the
//! default context -- when the request names none. The two carriers exclude
//! each other ("the presence of a JSON-LD Link header in the incoming HTTP
//! request when the Content-Type header is "application/ld+json" shall result
//! in an HTTP error response of type `BadRequestData`"), and a request naming
//! several context links is refused (JSON-LD 1.1 API section 9.4.1, "multiple
//! context link headers"). [`RequestContext`] is that choice, so a request can
//! never be read as having named both, or as having named several links.

use crate::context_value::{ContextEntry, context_entries};
use andromeda_json_literal::JsonLiteral;
use iri_rs::IriBuf;

/// The `@context` a request named, in the one place it named it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RequestContext {
    /// The request named no context: it is read against the default context,
    /// the core context alone (clause 5.5.5).
    #[default]
    Default,
    /// The request named its context in one `Link` header: the absolute IRI
    /// that header's target resolves to (IETF RFC 8288 section 3.1).
    Link(IriBuf),
    /// The request named its context in its body's `@context` member, as
    /// written: a reference, a context definition, or an array of those.
    Body(JsonLiteral),
}

impl RequestContext {
    /// The context IRI a `Link` header named, when the request named one.
    #[must_use]
    pub const fn link(&self) -> Option<&IriBuf> {
        match self {
            RequestContext::Link(url) => Some(url),
            RequestContext::Default | RequestContext::Body(_) => None,
        }
    }

    /// The body `@context` member, when the request carried one.
    #[must_use]
    pub const fn body(&self) -> Option<&JsonLiteral> {
        match self {
            RequestContext::Body(context) => Some(context),
            RequestContext::Default | RequestContext::Link(_) => None,
        }
    }

    /// The contexts the request named, in the order JSON-LD processes them:
    /// the one `Link` target as a reference, or the entries of the body
    /// member.
    #[must_use]
    pub fn entries(&self) -> Vec<ContextEntry<'_>> {
        match self {
            RequestContext::Default => Vec::new(),
            RequestContext::Link(url) => vec![ContextEntry::reference(url.as_str())],
            RequestContext::Body(context) => context_entries(context.piece()),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_value::{EntriesShape, written_entries},
        request_context::RequestContext,
    };
    use andromeda_json_literal::JsonLiteral;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn body(value: &serde_json::Value) -> RequestContext {
        RequestContext::Body(JsonLiteral::admit(&value.to_string()).expect("an admissible context"))
    }

    /// The entries `context` names, written as one array.
    fn entries(context: &RequestContext) -> serde_json::Value {
        serde_json::from_str(&written_entries(&context.entries(), EntriesShape::Array)).expect("the entries are JSON")
    }

    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    #[test]
    fn a_request_naming_no_context_has_no_entries() {
        let context = RequestContext::default();
        pretty_assertions::assert_eq!(context, RequestContext::Default);
        assert!(context.entries().is_empty());
        assert!(context.link().is_none());
        assert!(context.body().is_none());
    }

    #[test]
    fn a_link_context_is_its_one_target() {
        let context = RequestContext::Link(iri("https://example.com/a.jsonld"));
        pretty_assertions::assert_eq!(entries(&context), json!(["https://example.com/a.jsonld"]));
        pretty_assertions::assert_eq!(context.link().map(IriBuf::as_str), Some("https://example.com/a.jsonld"));
        assert!(context.body().is_none());
    }

    /// JSON-LD processes an `@context` array in order, repeats included.
    #[test]
    fn a_body_array_is_its_entries_in_order_with_repeats() {
        let context = body(&json!(["https://example.com/a.jsonld", {"t": "https://example.com/t"}, "https://example.com/a.jsonld"]));
        pretty_assertions::assert_eq!(
            entries(&context),
            json!(["https://example.com/a.jsonld", {"t": "https://example.com/t"}, "https://example.com/a.jsonld"])
        );
        assert!(context.link().is_none());
    }

    #[test]
    fn a_single_body_context_is_one_entry_and_null_is_none() {
        pretty_assertions::assert_eq!(entries(&body(&json!({"t": "https://example.com/t"}))), json!([{"t": "https://example.com/t"}]));
        assert!(body(&json!(null)).entries().is_empty());
    }
}
