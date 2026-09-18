//! The contexts that resolve without a network.
//!
//! Holds the embedded core contexts and whatever the operator preloaded from
//! disk. Documents are normalised once, at registration — a redundant embedded
//! Core @context reference is dropped per [`crate::core_context_precedence`] —
//! so a lookup on the expansion path is a map read and nothing else.

use crate::core_context_precedence::without_embedded_core_context;
use dashmap::{DashMap, DashSet};
use iri_rs::IriBuf;
use std::sync::Arc;

/// A thread-safe map from context URL to a parsed context document.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    /// The registered documents, shared by every clone of the registry.
    contexts: Arc<DashMap<IriBuf, Arc<jsonld_syntax::Value>>>,
    /// The URLs of documents the operator preloaded from disk, as distinct from
    /// the core contexts the broker embeds. Clause 5.13.1 flags "Cached" the
    /// copies the broker holds of documents whose authority is a URL it resolved,
    /// and a preloaded document is a local stand-in for exactly such a download,
    /// so the resolver records it durably as Cached while an embedded document is
    /// left out. The builder refuses to preload a core context URL, so a URL in
    /// this set is never an embedded context.
    preloaded: Arc<DashSet<IriBuf>>,
}

impl ContextRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> ContextRegistry {
        ContextRegistry::default()
    }

    /// Register a context document under a URL.
    ///
    /// A redundant embedded Core @context reference is dropped at registration
    /// (see [`without_embedded_core_context`]), so the broker's authoritative
    /// Core — appended last per clause 4.4 — stays the single occurrence and a
    /// registered user context that redefines a core term resolves to the Core's
    /// definition rather than failing.
    pub fn register(&self, url: IriBuf, value: jsonld_syntax::Value) {
        let normalized = without_embedded_core_context(&url, value);
        self.contexts.insert(url, Arc::new(normalized));
    }

    /// Register a context document the operator preloaded from disk.
    ///
    /// Behaves like [`Self::register`] but also records the URL as preloaded, so
    /// the resolver can tell a document it holds a downloaded-equivalent copy of
    /// (clause 5.13.1 "Cached") from one the broker embeds. The engine builder
    /// refuses to preload a core context URL, so this never records an embedded
    /// context as preloaded.
    pub fn register_preloaded(&self, url: IriBuf, value: jsonld_syntax::Value) {
        self.preloaded.insert(url.clone()); // clone: the set and the document map key on the same URL
        self.register(url, value);
    }

    /// The document registered for a URL, if there is one.
    #[must_use]
    pub fn get(&self, url: &IriBuf) -> Option<Arc<jsonld_syntax::Value>> {
        self.contexts.get(url).map(|entry| Arc::clone(entry.value()))
    }

    /// Whether the URL names a document the operator preloaded from disk, rather
    /// than one the broker embeds.
    #[must_use]
    pub fn is_preloaded(&self, url: &IriBuf) -> bool {
        self.preloaded.contains(url)
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_registry::ContextRegistry, core_context::NGSI_LD_CORE_CONTEXT_V19_URL};
    use iri_rs::IriBuf;
    use serde_json::json;

    #[test]
    fn a_registered_user_context_drops_its_redundant_core_import() {
        // Clause 5.5.7: the broker appends the authoritative Core last, giving
        // it absolute precedence regardless of position. A registered user
        // context's own embedded Core reference is therefore redundant and is
        // dropped, so the Core stays the single occurrence; the user context's
        // own new terms are kept.
        let registry = ContextRegistry::new();
        registry.register(
            IriBuf::new("https://example.com/user.jsonld".to_string()).expect("valid URL"),
            jsonld_syntax::Value::from_serde_json(json!({
                "@context": [NGSI_LD_CORE_CONTEXT_V19_URL, { "speed": "https://example.com/speed" }]
            })),
        );

        let url = IriBuf::new("https://example.com/user.jsonld".to_string()).expect("valid URL");
        let registered = registry.get(&url).expect("registered");

        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json((*registered).clone()),
            json!({ "@context": [{ "speed": "https://example.com/speed" }] })
        );
    }

    #[test]
    fn an_unregistered_url_resolves_to_nothing() {
        let url = IriBuf::new("https://example.com/absent.jsonld".to_string()).expect("valid URL");
        assert!(ContextRegistry::new().get(&url).is_none());
    }

    /// Clause 5.13.1: a document the operator preloaded is a local copy of one
    /// the broker would otherwise download, so the registry marks it preloaded;
    /// a document registered as embedded is not, and the resolver leaves it out
    /// of the Cached store on that basis.
    #[test]
    fn only_a_preloaded_document_is_marked_preloaded() {
        let registry = ContextRegistry::new();
        let preloaded = IriBuf::new("https://forge.example.com/suite.jsonld".to_string()).expect("valid URL");
        let embedded = IriBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_string()).expect("valid URL");

        registry.register_preloaded(preloaded.clone(), jsonld_syntax::Value::from_serde_json(json!({ "@context": {} })));
        registry.register(embedded.clone(), jsonld_syntax::Value::from_serde_json(json!({ "@context": {} })));

        assert!(registry.is_preloaded(&preloaded));
        assert!(!registry.is_preloaded(&embedded));
        assert!(registry.get(&preloaded).is_some());
        assert!(registry.get(&embedded).is_some());
    }
}
