//! Which `@context` applies to one operation.
//!
//! ETSI GS CIM 009 clause 5.5.7 lets a request carry its context in a `Link`
//! header, in the body, or not at all, and clause 4.3.6.8 lets a deployment
//! pin which released core context a versionless reference means.
//! [`ContextSpec`] is those three inputs plus the policy that decides how the
//! core context joins them; resolving it into a `@context` value is
//! [`crate::document_context`].

use crate::{
    core_context::{ANDROMEDA_CORE_CONTEXT_URL, is_ngsi_ld_core_context_url},
    document_context::resolve_context_value,
};
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::{Iri, IriBuf, IriRefBuf};
use serde_json::Value;
use std::collections::HashSet;

/// How the NGSI-LD core context joins the contexts a request supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreContextPolicy {
    /// Append the core context last, always.
    Append,
    /// Append it only when the request supplied no context of its own.
    AppendIfNoExplicit,
    /// Do not add it.
    Omit,
    /// Use it alone, discarding everything the request supplied.
    Only,
}

/// The contexts that apply to a single operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSpec {
    /// Context URLs the request carried in `Link` headers, in order.
    link_contexts: Vec<IriRefBuf>,
    /// The `@context` the request body carried, if any.
    body_context: Option<Value>,
    /// How the core context joins them.
    core_policy: CoreContextPolicy,
}

impl ContextSpec {
    /// A spec carrying nothing, with the core context appended.
    pub const fn none() -> ContextSpec {
        ContextSpec {
            link_contexts: Vec::new(),
            body_context: None,
            core_policy: CoreContextPolicy::Append,
        }
    }

    /// A spec that resolves to the core context alone.
    pub const fn core_only() -> ContextSpec {
        ContextSpec {
            link_contexts: Vec::new(),
            body_context: None,
            core_policy: CoreContextPolicy::Only,
        }
    }

    /// A spec built from the `Link` header context URLs of a request.
    pub const fn from_links(links: Vec<IriRefBuf>) -> ContextSpec {
        ContextSpec {
            link_contexts: links,
            body_context: None,
            core_policy: CoreContextPolicy::Append,
        }
    }

    /// Attach the `@context` the request body carried.
    #[must_use]
    pub fn with_body_context(mut self, context: Option<Value>) -> ContextSpec {
        self.body_context = context;
        self
    }

    /// The body `@context`, if the request carried one.
    pub const fn body_context(&self) -> Option<&Value> {
        self.body_context.as_ref()
    }

    /// Consume the spec and take its body `@context`.
    pub fn into_body_context(self) -> Option<Value> {
        self.body_context
    }

    /// The `Link` header context URLs, in the order the request gave them.
    pub fn link_contexts(&self) -> &[IriRefBuf] {
        &self.link_contexts
    }

    /// Choose how the core context joins the request's own contexts.
    #[must_use]
    pub const fn with_core_policy(mut self, policy: CoreContextPolicy) -> ContextSpec {
        self.core_policy = policy;
        self
    }

    /// The current core-context policy.
    pub const fn core_policy(&self) -> CoreContextPolicy {
        self.core_policy
    }

    /// The core context URL a response names for this spec.
    ///
    /// Always the versionless clause-4.3.6.8 core context: a response advertises
    /// the stable identifier so a client is not tied to the version this
    /// deployment happens to have pinned. Which released version a *versionless*
    /// reference expands against is a separate, engine-internal decision (the
    /// fastpath term table, [`crate::engine::JsonLdEngine::default_core_url`]),
    /// and it is not leaked here.
    pub const fn effective_core_url(&self) -> Iri<&'static str> {
        NGSI_LD_CORE_CONTEXT_URL
    }

    /// The same spec, as it applies to a response rather than a request.
    ///
    /// A response repeats the context the client supplied and names the core
    /// context only when the client named none, which is
    /// [`CoreContextPolicy::AppendIfNoExplicit`].
    pub fn for_presentation(&self) -> ContextSpec {
        ContextSpec {
            link_contexts: self.link_contexts.clone(), // clone: the source spec is borrowed and stays valid
            body_context: self.body_context.clone(),   // clone: the source spec is borrowed and stays valid
            core_policy: CoreContextPolicy::AppendIfNoExplicit,
        }
    }

    /// The `@context` value a response body carries for this spec.
    pub fn to_context_value(&self) -> Option<Value> {
        resolve_context_value(None, self)
    }

    /// The ordered context URLs a response carries in `Link` headers.
    ///
    /// Core-context aliases the request supplied are dropped and re-appended
    /// canonically, so a client that asked with v1.6 and a client that asked
    /// versionless receive the same header.
    pub fn to_link_headers(&self) -> Vec<IriRefBuf> {
        match self.core_policy {
            CoreContextPolicy::Only => return vec![core_context_iri_ref(NGSI_LD_CORE_CONTEXT_URL)],
            CoreContextPolicy::Omit | CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
        }

        let should_strip_core = matches!(self.core_policy, CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit);
        let mut urls = Vec::new();
        let mut seen = HashSet::new();

        for url in &self.link_contexts {
            if should_strip_core && (url.as_iri().is_some_and(is_ngsi_ld_core_context_url) || ANDROMEDA_CORE_CONTEXT_URL == url.as_str()) {
                continue;
            }
            if seen.insert(url.clone()) {
                // clone: the set keeps a copy, the list keeps a copy
                urls.push(url.clone());
            }
        }

        let has_explicit = !urls.is_empty() || self.body_context.is_some();
        let should_append_core = match self.core_policy {
            CoreContextPolicy::Append => true,
            CoreContextPolicy::AppendIfNoExplicit => !has_explicit,
            CoreContextPolicy::Omit | CoreContextPolicy::Only => false,
        };

        if should_append_core {
            let andromeda_url = core_context_iri_ref(ANDROMEDA_CORE_CONTEXT_URL);
            if seen.insert(andromeda_url.clone()) {
                // clone: the set keeps a copy, the list keeps a copy
                urls.push(andromeda_url);
            }
            let core_url = core_context_iri_ref(NGSI_LD_CORE_CONTEXT_URL);
            if seen.insert(core_url.clone()) {
                // clone: the set keeps a copy, the list keeps a copy
                urls.push(core_url);
            }
        }

        urls
    }
}

/// The header form of a compile-time-valid core context IRI.
///
/// The core context URLs a response advertises are compile-time constants, so
/// this conversion is infallible; it bridges the `Iri` a constant carries to the
/// owned [`IriRefBuf`] a `Link` header value holds.
fn core_context_iri_ref(iri: Iri<&'static str>) -> IriRefBuf {
    IriRefBuf::from(IriBuf::from(iri))
}

impl Default for ContextSpec {
    fn default() -> ContextSpec {
        ContextSpec::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::{ContextSpec, CoreContextPolicy},
        core_context::{ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriRefBuf;
    use serde_json::json;

    /// A context URL as the [`IriRefBuf`] the `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    #[test]
    fn an_empty_spec_resolves_to_the_andromeda_and_core_contexts() {
        let context = ContextSpec::none().to_context_value().expect("a context value");
        pretty_assertions::assert_eq!(context, json!([ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn link_headers_carry_the_request_context_first_and_the_core_contexts_last() {
        let links = ContextSpec::from_links(vec![iri_ref("https://example.com/context.jsonld")]).to_link_headers();
        pretty_assertions::assert_eq!(
            links,
            vec![
                iri_ref("https://example.com/context.jsonld"),
                iri_ref(ANDROMEDA_CORE_CONTEXT_URL.as_str()),
                iri_ref(NGSI_LD_CORE_CONTEXT_URL.as_str()),
            ]
        );
    }

    #[test]
    fn the_only_policy_discards_every_context_the_request_supplied() {
        let spec = ContextSpec::from_links(vec![iri_ref("https://example.com/context.jsonld")]).with_core_policy(CoreContextPolicy::Only);
        pretty_assertions::assert_eq!(spec.to_link_headers(), vec![iri_ref(NGSI_LD_CORE_CONTEXT_URL.as_str())]);
    }

    #[test]
    fn a_core_only_spec_resolves_to_the_bare_core_url() {
        let spec = ContextSpec::core_only();
        pretty_assertions::assert_eq!(spec.to_context_value().expect("a context value"), json!(NGSI_LD_CORE_CONTEXT_URL));
        pretty_assertions::assert_eq!(spec.core_policy(), CoreContextPolicy::Only);
    }

    #[test]
    fn a_core_context_alias_the_request_supplied_comes_back_canonical() {
        let links = ContextSpec::from_links(vec![
            iri_ref("https://example.com/context.jsonld"),
            iri_ref(NGSI_LD_CORE_CONTEXT_V16_URL.as_str()),
        ])
        .to_link_headers();

        pretty_assertions::assert_eq!(
            links,
            vec![
                iri_ref("https://example.com/context.jsonld"),
                iri_ref(ANDROMEDA_CORE_CONTEXT_URL.as_str()),
                iri_ref(NGSI_LD_CORE_CONTEXT_URL.as_str()),
            ]
        );
    }

    #[test]
    fn a_presentation_spec_names_the_core_context_only_when_the_request_named_none() {
        let bare = ContextSpec::none().for_presentation();
        pretty_assertions::assert_eq!(
            bare.to_link_headers(),
            vec![iri_ref(ANDROMEDA_CORE_CONTEXT_URL.as_str()), iri_ref(NGSI_LD_CORE_CONTEXT_URL.as_str())]
        );

        let explicit = ContextSpec::from_links(vec![iri_ref("https://example.com/context.jsonld")]).for_presentation();
        pretty_assertions::assert_eq!(explicit.to_link_headers(), vec![iri_ref("https://example.com/context.jsonld")]);
    }
}
