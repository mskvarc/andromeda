//! Which `@context` applies to one operation.
//!
//! ETSI GS CIM 009 clause 6.3.5 lets a request name its context in exactly one
//! place -- a `Link` header, the body, or nowhere -- which is
//! [`RequestContext`]. [`ContextSpec`] is that choice plus the policy that
//! decides how the core context joins it; resolving it into the `@context` a
//! document is processed under is [`crate::document_context`], and into the
//! `@context` a response names is [`crate::presented_context`].

use crate::{
    core_context::is_ngsi_ld_core_context_url,
    document_context::{ReferenceReading, ResolvedContext, resolved_context},
    request_context::RequestContext,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::{Iri, IriBuf};
use std::sync::Arc;

/// How the NGSI-LD core context joins the context a request named when a
/// document is processed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CoreContextPolicy {
    /// Append the core context last, always (clauses 4.4 and 5.5.7).
    #[default]
    Append,
    /// Use it alone, discarding whatever the request named.
    Only,
}

/// The context that applies to a single operation.
///
/// The default is [`ContextSpec::none`]: no context named, the core context
/// appended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextSpec {
    /// Where the request named its context, if anywhere.
    context: RequestContext,
    /// How the core context joins it.
    core_policy: CoreContextPolicy,
}

impl ContextSpec {
    /// A spec for a request that named no context, with the core context
    /// appended.
    #[must_use]
    pub const fn none() -> ContextSpec {
        ContextSpec {
            context: RequestContext::Default,
            core_policy: CoreContextPolicy::Append,
        }
    }

    /// A spec that resolves to the core context alone, for a resource that
    /// always answers under it whatever the request named.
    #[must_use]
    pub const fn core_only() -> ContextSpec {
        ContextSpec {
            context: RequestContext::Default,
            core_policy: CoreContextPolicy::Only,
        }
    }

    /// A spec for the context a request named.
    #[must_use]
    pub const fn from_request(context: RequestContext) -> ContextSpec {
        ContextSpec {
            context,
            core_policy: CoreContextPolicy::Append,
        }
    }

    /// A spec for a request whose one `Link` header named `url`.
    #[must_use]
    pub const fn from_link(url: IriBuf) -> ContextSpec {
        ContextSpec::from_request(RequestContext::Link(url))
    }

    /// A spec for a request whose body named `context` as its `@context`.
    #[must_use]
    pub const fn from_body(context: JsonLiteral) -> ContextSpec {
        ContextSpec::from_request(RequestContext::Body(context))
    }

    /// The spec a subscription's `jsonldContext` selects for its notifications
    /// (clause 5.8.6): that context with the core context appended, or the
    /// core context alone when the subscription named none.
    #[must_use]
    pub fn for_subscription_context(jsonld_context: Option<&IriBuf>) -> ContextSpec {
        match jsonld_context {
            Some(context) => ContextSpec::from_link(context.clone()), // clone: the subscription keeps its jsonldContext
            None => ContextSpec::none(),
        }
    }

    /// Where the request named its context.
    #[must_use]
    pub const fn request_context(&self) -> &RequestContext {
        &self.context
    }

    /// The context IRI the request's one `Link` header named, if it named one.
    #[must_use]
    pub const fn link_context(&self) -> Option<&IriBuf> {
        self.context.link()
    }

    /// The body `@context`, if the request carried one.
    #[must_use]
    pub const fn body_context(&self) -> Option<&JsonLiteral> {
        self.context.body()
    }

    /// The current core-context policy.
    #[must_use]
    pub const fn core_policy(&self) -> CoreContextPolicy {
        self.core_policy
    }

    /// The core context URL a document is processed under.
    ///
    /// Always the v1.9 core context this broker implements (clause 4.4): every
    /// core reference a request names, of any release or the versionless
    /// alias, is removed and this one appended last, so the documents a
    /// request names are processed under the same definitions whichever
    /// release the client wrote.
    #[must_use]
    pub const fn effective_core_url(&self) -> Iri<&'static str> {
        NGSI_LD_CORE_CONTEXT_URL
    }

    /// The `@context` a document is processed under for this spec, its
    /// references as written.
    #[must_use]
    pub fn resolved_context(&self) -> ResolvedContext {
        resolved_context(self, ReferenceReading::AsWritten)
    }

    /// The key a process-wide cache keeps the context this spec processes to
    /// under, or `None` when that processed context must not outlive one
    /// operation.
    ///
    /// A spec qualifies when it carries no body `@context` and every `Link`
    /// context it names is a released core context, all of which are embedded
    /// in the broker: an embedded context cannot change, while a context
    /// fetched from a URL carries the lifetime its origin granted (clause
    /// 6.3.16), which the loader enforces each time it resolves it.
    #[must_use]
    pub fn embedded_context_key(&self) -> Option<Arc<str>> {
        let embedded_only = self.body_context().is_none() && self.link_context().is_none_or(|url| is_ngsi_ld_core_context_url(url.as_ref()));
        embedded_only.then(|| self.context_key())
    }

    /// The key one engine keeps the context this spec processes to under, for
    /// as long as the documents it was processed from stay what the loader
    /// resolves (`engine::processed_context_cache`).
    ///
    /// The key is the resolved `@context` text, which is exactly what gets
    /// processed, so two specs share a key only when they would process the
    /// same value.
    #[must_use]
    pub fn context_key(&self) -> Arc<str> {
        self.resolved_context().key()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::{ContextSpec, CoreContextPolicy},
        core_context::NGSI_LD_CORE_CONTEXT_V16_URL,
        request_context::RequestContext,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::json;

    fn body(value: &serde_json::Value) -> ContextSpec {
        ContextSpec::from_body(literal(value))
    }

    fn literal(value: &serde_json::Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible context")
    }

    /// The resolved context of `spec`, as JSON.
    fn resolved(spec: &ContextSpec) -> serde_json::Value {
        serde_json::from_str(spec.resolved_context().text()).expect("the resolved context is JSON")
    }

    /// A context URL as the absolute IRI a `Link` header resolves to.
    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    #[test]
    fn a_subscription_context_is_the_named_context_with_the_core_context_appended() {
        let url = iri("https://example.com/context.jsonld");
        let named = ContextSpec::for_subscription_context(Some(&url));
        pretty_assertions::assert_eq!(named.link_context(), Some(&url));
        pretty_assertions::assert_eq!(named.core_policy(), CoreContextPolicy::Append);
        pretty_assertions::assert_eq!(ContextSpec::for_subscription_context(None), ContextSpec::none());
    }

    #[test]
    fn an_empty_spec_resolves_to_the_core_context() {
        pretty_assertions::assert_eq!(resolved(&ContextSpec::none()), json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_link_context_is_processed_first_and_the_core_context_last() {
        let context = resolved(&ContextSpec::from_link(iri("https://example.com/context.jsonld")));
        pretty_assertions::assert_eq!(context, json!(["https://example.com/context.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn the_default_spec_names_no_context_and_appends_the_core_context() {
        pretty_assertions::assert_eq!(ContextSpec::default(), ContextSpec::none());
        pretty_assertions::assert_eq!(ContextSpec::default().core_policy(), CoreContextPolicy::Append);
    }

    #[test]
    fn a_core_only_spec_resolves_to_the_core_url_array() {
        let spec = ContextSpec::core_only();
        pretty_assertions::assert_eq!(resolved(&spec), json!([NGSI_LD_CORE_CONTEXT_URL]));
        pretty_assertions::assert_eq!(spec.core_policy(), CoreContextPolicy::Only);
    }

    /// Clause 6.3.5: a request names its context in one place, so a spec holds
    /// a `Link` context or a body context, never both.
    #[test]
    fn a_spec_holds_its_context_in_exactly_one_place() {
        let linked = ContextSpec::from_link(iri("https://example.com/a.jsonld"));
        assert!(linked.body_context().is_none());
        pretty_assertions::assert_eq!(linked.request_context(), &RequestContext::Link(iri("https://example.com/a.jsonld")));

        let bodied = body(&json!("https://example.com/a.jsonld"));
        assert!(bodied.link_context().is_none());
        pretty_assertions::assert_eq!(bodied.body_context(), Some(&literal(&json!("https://example.com/a.jsonld"))));

        pretty_assertions::assert_eq!(ContextSpec::none().request_context(), &RequestContext::Default);
    }

    /// Clause 4.4: a core context the request named in another release is
    /// processed as the v1.9 one, last.
    #[test]
    fn a_core_context_alias_the_request_supplied_is_processed_as_the_current_one_last() {
        let context = resolved(&body(&json!([NGSI_LD_CORE_CONTEXT_V16_URL.as_str(), "https://example.com/context.jsonld"])));

        pretty_assertions::assert_eq!(context, json!(["https://example.com/context.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// A `Link` naming a core context of another release is processed as the
    /// v1.9 core context alone.
    #[test]
    fn a_link_naming_an_older_core_context_resolves_to_the_current_core_context() {
        let context = resolved(&ContextSpec::from_link(iri(NGSI_LD_CORE_CONTEXT_V16_URL.as_str())));
        pretty_assertions::assert_eq!(context, json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_spec_naming_no_context_has_an_embedded_context_key() {
        assert!(ContextSpec::none().embedded_context_key().is_some());
        assert!(ContextSpec::core_only().embedded_context_key().is_some());
    }

    #[test]
    fn a_spec_naming_only_released_core_contexts_has_an_embedded_context_key() {
        assert!(
            ContextSpec::from_link(iri(NGSI_LD_CORE_CONTEXT_V16_URL.as_str()))
                .embedded_context_key()
                .is_some()
        );
    }

    /// Clause 6.3.16: a fetched context carries its own lifetime, so nothing
    /// processed from it is kept past one operation.
    #[test]
    fn a_spec_naming_a_remote_context_has_no_embedded_context_key() {
        assert!(
            ContextSpec::from_link(iri("https://example.com/context.jsonld"))
                .embedded_context_key()
                .is_none()
        );
    }

    #[test]
    fn a_spec_carrying_a_body_context_has_no_embedded_context_key() {
        let spec = body(&json!({ "speed": "https://example.com/speed" }));
        assert!(spec.embedded_context_key().is_none());
    }

    #[test]
    fn specs_resolving_to_the_same_context_share_one_embedded_context_key() {
        let versioned = ContextSpec::from_link(iri(NGSI_LD_CORE_CONTEXT_V16_URL.as_str()));
        pretty_assertions::assert_eq!(versioned.embedded_context_key(), ContextSpec::none().embedded_context_key());
    }
}
