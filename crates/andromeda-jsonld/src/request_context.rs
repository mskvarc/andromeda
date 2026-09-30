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

use iri_rs::IriBuf;
use serde_json::Value;

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
    Body(Value),
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
    pub const fn body(&self) -> Option<&Value> {
        match self {
            RequestContext::Body(context) => Some(context),
            RequestContext::Default | RequestContext::Link(_) => None,
        }
    }

    /// The contexts the request named, in the order JSON-LD processes them,
    /// each written as the `@context` entry it is: the one `Link` target as a
    /// string, or the entries of the body member.
    #[must_use]
    pub fn entries(&self) -> Vec<Value> {
        match self {
            RequestContext::Default | RequestContext::Body(Value::Null) => Vec::new(),
            RequestContext::Link(url) => vec![Value::String(url.as_str().to_owned())],
            RequestContext::Body(Value::Array(entries)) => entries.clone(), // clone: the request keeps the context it named
            RequestContext::Body(context @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_))) => vec![context.clone()], // clone: the request keeps the context it named
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::request_context::RequestContext;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    #[test]
    fn a_request_naming_no_context_has_no_entries() {
        let context = RequestContext::default();
        pretty_assertions::assert_eq!(context, RequestContext::Default);
        pretty_assertions::assert_eq!(context.entries(), Vec::<serde_json::Value>::new());
        assert!(context.link().is_none());
        assert!(context.body().is_none());
    }

    #[test]
    fn a_link_context_is_its_one_target() {
        let context = RequestContext::Link(iri("https://example.com/a.jsonld"));
        pretty_assertions::assert_eq!(context.entries(), vec![json!("https://example.com/a.jsonld")]);
        pretty_assertions::assert_eq!(context.link().map(IriBuf::as_str), Some("https://example.com/a.jsonld"));
        assert!(context.body().is_none());
    }

    /// JSON-LD processes an `@context` array in order, repeats included.
    #[test]
    fn a_body_array_is_its_entries_in_order_with_repeats() {
        let context = RequestContext::Body(json!(["https://example.com/a.jsonld", {"t": "https://example.com/t"}, "https://example.com/a.jsonld"]));
        pretty_assertions::assert_eq!(
            context.entries(),
            vec![
                json!("https://example.com/a.jsonld"),
                json!({"t": "https://example.com/t"}),
                json!("https://example.com/a.jsonld")
            ]
        );
        assert!(context.link().is_none());
    }

    #[test]
    fn a_single_body_context_is_one_entry_and_null_is_none() {
        pretty_assertions::assert_eq!(
            RequestContext::Body(json!({"t": "https://example.com/t"})).entries(),
            vec![json!({"t": "https://example.com/t"})]
        );
        pretty_assertions::assert_eq!(RequestContext::Body(json!(null)).entries(), Vec::<serde_json::Value>::new());
    }
}
