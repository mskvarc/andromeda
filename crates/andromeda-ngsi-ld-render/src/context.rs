//! Where a response's `@context` goes, and what it says.
//!
//! Clause 6.3.6 gives the JSON-LD `@context` of a response two carriers: an
//! `application/ld+json` body holds it as a member, and an `application/json`
//! body cannot, so it is advertised through `Link` headers instead (clause
//! 6.3.6). Which carrier applies is decided by the media type and, for GeoJSON,
//! by the `Prefer: body=` token — so the renderer decides it, and both
//! spellings have to be ready before it does.
//!
//! Resolving *which* contexts apply is not a rendering question and is not
//! here: it depends on what the request carried, on the broker's core-context
//! policy and on what it has cached. The caller resolves that once and hands
//! the answer down as a [`ResponseContext`].

use iri_rs::IriRefBuf;
use serde_json::Value;

/// Where the JSON-LD `@context` appears in a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLocation {
    /// Advertised through `Link` headers (clause 6.3.6).
    LinkHeader,
    /// Embedded in the JSON body.
    Body,
}

/// A response's `@context`, in both spellings the carriers need.
///
/// Built once from whatever resolved the contexts, because the two readings
/// must describe the same set: a body that names one context and a `Link`
/// header that names another is a response no client can interpret twice the
/// same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseContext {
    /// The value to embed when the context goes in the body, if there is one.
    body_value: Option<Value>,
    /// The context URLs to advertise when it goes in the headers.
    link_urls: Vec<IriRefBuf>,
}

impl ResponseContext {
    /// Build a response context from its two resolved spellings.
    pub const fn new(body_value: Option<Value>, link_urls: Vec<IriRefBuf>) -> ResponseContext {
        ResponseContext { body_value, link_urls }
    }

    /// The context URLs a `Link` header should advertise.
    pub fn link_urls(&self) -> &[IriRefBuf] {
        &self.link_urls
    }

    /// Take the body value, formatted for embedding.
    ///
    /// The value is embedded exactly as resolved. Clause 6.3.6 requires the
    /// response `@context`; always emitting it as a JSON-LD array, including the
    /// single-context case, is Andromeda's chosen uniform shape rather than a
    /// spec mandate, so it is never collapsed to a bare string.
    pub const fn take_body_value(&mut self) -> Option<Value> {
        self.body_value.take()
    }
}

#[cfg(test)]
mod tests {
    use crate::context::ResponseContext;
    use iri_rs::IriRefBuf;
    use serde_json::json;

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    #[test]
    fn a_single_context_is_embedded_as_a_one_element_array() {
        let mut context = ResponseContext::new(Some(json!(["https://example.com/a"])), Vec::new());
        pretty_assertions::assert_eq!(context.take_body_value(), Some(json!(["https://example.com/a"])));
    }

    #[test]
    fn several_contexts_are_embedded_as_an_array() {
        let contexts = json!(["https://example.com/a", "https://example.com/b"]);
        let mut context = ResponseContext::new(Some(contexts.clone()), Vec::new());
        pretty_assertions::assert_eq!(context.take_body_value(), Some(contexts));
    }

    #[test]
    fn an_absent_context_stays_absent() {
        let mut context = ResponseContext::new(None, vec![iri_ref("https://example.com/a")]);
        pretty_assertions::assert_eq!(context.take_body_value(), None);
        pretty_assertions::assert_eq!(context.link_urls(), [iri_ref("https://example.com/a")]);
    }

    #[test]
    fn the_body_value_is_taken_once_and_the_link_urls_survive_it() {
        let mut context = ResponseContext::new(Some(json!("https://example.com/a")), vec![iri_ref("https://example.com/a")]);
        assert!(context.take_body_value().is_some());
        pretty_assertions::assert_eq!(context.take_body_value(), None);
        pretty_assertions::assert_eq!(context.link_urls(), [iri_ref("https://example.com/a")]);
    }
}
