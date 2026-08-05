//! Which `@context` applies to one operation.
//!
//! ETSI GS CIM 009 clause 5.5.7 lets a request carry its context in a `Link`
//! header, in the body, or not at all, and clause 4.3.6.8 lets a deployment
//! pin which released core context a versionless reference means.
//! [`ContextSpec`] is those three inputs plus the policy that decides how the
//! core context joins them; resolving it into a `@context` value is
//! [`crate::document_context`].

use crate::{
    core_context::{ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL, is_ngsi_ld_core_context_url},
    document_context::resolve_context_value,
};
use serde_json::Value;
use std::{collections::HashSet, sync::OnceLock};

/// The deployment-wide core context URL, set once at startup.
static DEFAULT_CORE_CONTEXT_URL: OnceLock<String> = OnceLock::new();

/// Pin which core context version a versionless reference resolves to.
///
/// Clause 4.3.6.8 admits any released core context; a deployment picks one.
/// The first call wins — a second is ignored with a warning, because a
/// process that changed the answer halfway would expand two otherwise
/// identical requests differently.
pub fn set_default_core_context_url(url: impl Into<String>) {
    if DEFAULT_CORE_CONTEXT_URL.set(url.into()).is_err() {
        tracing::warn!(
            "set_default_core_context_url called more than once; keeping the original value '{}'",
            DEFAULT_CORE_CONTEXT_URL.get().map_or("<unknown>", String::as_str)
        );
    }
}

/// The pinned core context URL, or the versionless one if none was pinned.
pub fn default_core_context_url() -> &'static str {
    DEFAULT_CORE_CONTEXT_URL.get().map_or(NGSI_LD_CORE_CONTEXT_URL, String::as_str)
}

/// The pinned core context URL, if one was pinned.
fn global_default_core_url() -> Option<String> {
    DEFAULT_CORE_CONTEXT_URL.get().cloned() // clone: the spec owns its copy for the process lifetime
}

/// How the NGSI-LD core context joins the contexts a request supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    link_contexts: Vec<String>,
    /// The `@context` the request body carried, if any.
    body_context: Option<Value>,
    /// How the core context joins them.
    core_policy: CoreContextPolicy,
    /// The core context URL this spec resolves versionless references to.
    default_core_url: Option<String>,
}

impl ContextSpec {
    /// A spec carrying nothing, with the core context appended.
    pub fn none() -> ContextSpec {
        ContextSpec {
            link_contexts: Vec::new(),
            body_context: None,
            core_policy: CoreContextPolicy::Append,
            default_core_url: global_default_core_url(),
        }
    }

    /// A spec that resolves to the core context alone.
    pub fn core_only() -> ContextSpec {
        ContextSpec {
            link_contexts: Vec::new(),
            body_context: None,
            core_policy: CoreContextPolicy::Only,
            default_core_url: global_default_core_url(),
        }
    }

    /// A spec built from the `Link` header context URLs of a request.
    pub fn from_links(links: Vec<String>) -> ContextSpec {
        ContextSpec {
            link_contexts: links,
            body_context: None,
            core_policy: CoreContextPolicy::Append,
            default_core_url: global_default_core_url(),
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
    pub fn link_contexts(&self) -> &[String] {
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

    /// The core context URL this spec resolves to.
    pub fn effective_core_url(&self) -> &str {
        self.default_core_url.as_deref().unwrap_or(NGSI_LD_CORE_CONTEXT_URL)
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
            default_core_url: self.default_core_url.clone(), // clone: the source spec is borrowed and stays valid
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
    pub fn to_link_headers(&self) -> Vec<String> {
        let core_url = self.effective_core_url().to_string();

        match self.core_policy {
            CoreContextPolicy::Only => return vec![core_url],
            CoreContextPolicy::Omit | CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
        }

        let should_strip_core = matches!(self.core_policy, CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit);
        let mut urls = Vec::new();
        let mut seen = HashSet::new();

        for url in &self.link_contexts {
            if should_strip_core && (is_ngsi_ld_core_context_url(url) || url == ANDROMEDA_CORE_CONTEXT_URL) {
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
            let andromeda_url = ANDROMEDA_CORE_CONTEXT_URL.to_string();
            if seen.insert(andromeda_url.clone()) {
                // clone: the set keeps a copy, the list keeps a copy
                urls.push(andromeda_url);
            }
            if seen.insert(core_url.clone()) {
                // clone: the set keeps a copy, the list keeps a copy
                urls.push(core_url);
            }
        }

        urls
    }
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
        core_context::{ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
    };
    use serde_json::json;

    #[test]
    fn an_empty_spec_resolves_to_the_andromeda_and_core_contexts() {
        let context = ContextSpec::none().to_context_value().expect("a context value");
        pretty_assertions::assert_eq!(context, json!([ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn link_headers_carry_the_request_context_first_and_the_core_contexts_last() {
        let links = ContextSpec::from_links(vec!["https://example.com/context.jsonld".to_string()]).to_link_headers();
        pretty_assertions::assert_eq!(
            links,
            vec![
                "https://example.com/context.jsonld".to_string(),
                ANDROMEDA_CORE_CONTEXT_URL.to_string(),
                NGSI_LD_CORE_CONTEXT_URL.to_string(),
            ]
        );
    }

    #[test]
    fn the_only_policy_discards_every_context_the_request_supplied() {
        let spec = ContextSpec::from_links(vec!["https://example.com/context.jsonld".to_string()]).with_core_policy(CoreContextPolicy::Only);
        pretty_assertions::assert_eq!(spec.to_link_headers(), vec![NGSI_LD_CORE_CONTEXT_URL.to_string()]);
    }

    #[test]
    fn a_core_only_spec_resolves_to_the_bare_core_url() {
        let spec = ContextSpec::core_only();
        pretty_assertions::assert_eq!(spec.to_context_value().expect("a context value"), json!(NGSI_LD_CORE_CONTEXT_URL));
        pretty_assertions::assert_eq!(spec.core_policy(), CoreContextPolicy::Only);
    }

    #[test]
    fn a_core_context_alias_the_request_supplied_comes_back_canonical() {
        let links = ContextSpec::from_links(vec!["https://example.com/context.jsonld".to_string(), NGSI_LD_CORE_CONTEXT_V16_URL.to_string()]).to_link_headers();

        pretty_assertions::assert_eq!(
            links,
            vec![
                "https://example.com/context.jsonld".to_string(),
                ANDROMEDA_CORE_CONTEXT_URL.to_string(),
                NGSI_LD_CORE_CONTEXT_URL.to_string(),
            ]
        );
    }

    #[test]
    fn a_presentation_spec_names_the_core_context_only_when_the_request_named_none() {
        let bare = ContextSpec::none().for_presentation();
        pretty_assertions::assert_eq!(
            bare.to_link_headers(),
            vec![ANDROMEDA_CORE_CONTEXT_URL.to_string(), NGSI_LD_CORE_CONTEXT_URL.to_string()]
        );

        let explicit = ContextSpec::from_links(vec!["https://example.com/context.jsonld".to_string()]).for_presentation();
        pretty_assertions::assert_eq!(explicit.to_link_headers(), vec!["https://example.com/context.jsonld".to_string()]);
    }
}
