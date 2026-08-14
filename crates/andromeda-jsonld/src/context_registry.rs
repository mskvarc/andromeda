//! The contexts that resolve without a network.
//!
//! Holds the embedded core contexts and whatever the operator preloaded from
//! disk. Documents are normalised once, at registration — a redundant embedded
//! Core @context reference is dropped per [`crate::core_context_precedence`] —
//! so a lookup on the expansion path is a map read and nothing else.

use crate::core_context_precedence::without_embedded_core_context;
use dashmap::DashMap;
use iri_rs::IriBuf;
use std::sync::Arc;

/// A thread-safe map from context URL to a parsed context document.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    /// The registered documents, shared by every clone of the registry.
    contexts: Arc<DashMap<IriBuf, Arc<jsonld_syntax::Value>>>,
}

impl ContextRegistry {
    /// An empty registry.
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

    /// The document registered for a URL, if there is one.
    pub fn get(&self, url: &IriBuf) -> Option<Arc<jsonld_syntax::Value>> {
        self.contexts.get(url).map(|entry| Arc::clone(entry.value()))
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
}
