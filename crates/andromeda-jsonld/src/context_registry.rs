//! The contexts that resolve without a network.
//!
//! Holds the embedded core contexts and whatever the operator preloaded from
//! disk. Documents are normalised once, at registration, so a lookup on the
//! expansion path is a map read and nothing else.

use crate::loader::url_normalization::normalize_loaded_context_document;
use dashmap::DashMap;
use std::sync::Arc;

/// A thread-safe map from context URL to a parsed context document.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    /// The registered documents, shared by every clone of the registry.
    contexts: Arc<DashMap<String, Arc<jsonld_syntax::Value>>>,
}

impl ContextRegistry {
    /// An empty registry.
    pub fn new() -> ContextRegistry {
        ContextRegistry::default()
    }

    /// Register a context document under a URL.
    ///
    /// Normalisation happens here rather than per lookup, so a registered
    /// document is already free of the embedded core-context references the
    /// processor would otherwise resolve a second time.
    pub fn register(&self, url: impl Into<String>, value: jsonld_syntax::Value) {
        let url = url.into();
        let normalized = normalize_loaded_context_document(&url, value);
        self.contexts.insert(url, Arc::new(normalized));
    }

    /// The document registered for a URL, if there is one.
    pub fn get(&self, url: &str) -> Option<Arc<jsonld_syntax::Value>> {
        self.contexts.get(url).map(|entry| Arc::clone(entry.value()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_registry::ContextRegistry, core_context::NGSI_LD_CORE_CONTEXT_V19_URL};
    use serde_json::json;

    #[test]
    fn a_registered_user_context_loses_its_core_context_import_at_registration_time() {
        let registry = ContextRegistry::new();
        registry.register(
            "https://example.com/user.jsonld",
            jsonld_syntax::Value::from_serde_json(json!({
                "@context": [NGSI_LD_CORE_CONTEXT_V19_URL, { "speed": "https://example.com/speed" }]
            })),
        );

        let registered = registry.get("https://example.com/user.jsonld").expect("registered");

        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json((*registered).clone()),
            json!({ "@context": [{ "speed": "https://example.com/speed" }] })
        );
    }

    #[test]
    fn an_unregistered_url_resolves_to_nothing() {
        assert!(ContextRegistry::new().get("https://example.com/absent.jsonld").is_none());
    }
}
