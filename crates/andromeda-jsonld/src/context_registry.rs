//! The contexts that resolve without a network.
//!
//! Holds the embedded core contexts and whatever the operator preloaded from
//! disk. Documents are normalised once, at registration — a redundant embedded
//! Core @context reference is dropped per [`crate::core_context_precedence`] —
//! so a lookup on the expansion path is a map read and nothing else.

use crate::{context_tail::ContextTail, core_context_precedence::prepared_context};
use dashmap::DashMap;
use iri_rs::IriBuf;
use std::sync::Arc;

/// One registered document: the copy the JSON-LD processor reads, and what the
/// document as written leaves in force last.
#[derive(Debug, Clone)]
pub struct RegisteredContext {
    /// The document with its embedded Core @context references removed.
    pub value: Arc<jsonld_syntax::Value>,
    /// What the document as written leaves in force last (clause 4.4).
    pub tail: ContextTail,
}

/// A thread-safe map from context URL to a parsed context document.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    /// The registered documents, shared by every clone of the registry.
    contexts: Arc<DashMap<IriBuf, RegisteredContext>>,
    /// The documents the operator preloaded from disk, as written, by URL, as
    /// distinct from the core contexts the broker embeds. Clause 5.13.1 flags
    /// "Cached" the copies the broker holds of documents whose authority is a
    /// URL it resolved, and a preloaded document is a local stand-in for exactly
    /// such a download, so the resolver records it durably as Cached -- as
    /// written, like every download -- while an embedded document is left out.
    /// The builder refuses to preload a core context URL, so a URL here is never
    /// an embedded context.
    preloaded: Arc<DashMap<IriBuf, Arc<jsonld_syntax::Value>>>,
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
    /// (see [`prepared_context`]), so the broker's authoritative Core — appended
    /// last per clause 4.4 — stays the single occurrence and a registered user
    /// context that redefines a core term resolves to the Core's definition
    /// rather than failing.
    pub fn register(&self, url: IriBuf, value: jsonld_syntax::Value) {
        let prepared = prepared_context(&url, jsonld_syntax::Value::into_serde_json(value));
        self.contexts.insert(
            url,
            RegisteredContext {
                value: Arc::new(prepared.value),
                tail: prepared.tail,
            },
        );
    }

    /// Register a context document the operator preloaded from disk.
    ///
    /// Behaves like [`Self::register`] but also keeps the document as written,
    /// so the resolver can tell a document it holds a downloaded-equivalent copy
    /// of (clause 5.13.1 "Cached") from one the broker embeds, and records that
    /// copy as the document was written rather than as it is prepared for
    /// processing. The engine builder refuses to preload a core context URL, so
    /// this never records an embedded context as preloaded.
    pub fn register_preloaded(&self, url: IriBuf, value: jsonld_syntax::Value) {
        // clone: the preloaded map keeps the document as written while the processor reads the prepared copy
        self.preloaded.insert(url.clone(), Arc::new(value.clone())); // clone: both maps key on the same URL
        self.register(url, value);
    }

    /// The document registered for a URL, if there is one.
    #[must_use]
    pub fn get(&self, url: &IriBuf) -> Option<RegisteredContext> {
        self.contexts.get(url).map(|entry| entry.value().clone()) // clone: an `Arc` bump and the tail, handed out while the map keeps its entry
    }

    /// The document the operator preloaded from disk under `url`, as written,
    /// or `None` for a URL the broker embeds or holds nothing under.
    #[must_use]
    pub fn preloaded_document(&self, url: &IriBuf) -> Option<Arc<jsonld_syntax::Value>> {
        self.preloaded.get(url).map(|entry| Arc::clone(entry.value()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_registry::ContextRegistry, context_tail::ContextTail};
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
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
                "@context": [NGSI_LD_CORE_CONTEXT_URL, { "speed": "https://example.com/speed" }]
            })),
        );

        let url = IriBuf::new("https://example.com/user.jsonld".to_string()).expect("valid URL");
        let registered = registry.get(&url).expect("registered");

        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json((*registered.value).clone()),
            json!({ "@context": [{ "speed": "https://example.com/speed" }] })
        );
    }

    /// Clause 4.4 and Annex C.8: the registry keeps what the document as
    /// written left in force last, although the copy it hands the processor
    /// no longer names the Core.
    #[test]
    fn a_registered_document_keeps_what_it_left_in_force_last() {
        let registry = ContextRegistry::new();
        let url = IriBuf::new("https://example.com/compound.jsonld".to_string()).expect("valid URL");
        registry.register(
            url.clone(), // clone: the registry keys the entry on the URL the lookup below names
            jsonld_syntax::Value::from_serde_json(json!({ "@context": ["https://example.com/terms.jsonld", NGSI_LD_CORE_CONTEXT_URL] })),
        );

        pretty_assertions::assert_eq!(registry.get(&url).map(|registered| registered.tail), Some(ContextTail::CurrentCore));
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

        assert!(registry.preloaded_document(&preloaded).is_some());
        assert!(registry.preloaded_document(&embedded).is_none());
        assert!(registry.get(&preloaded).is_some());
        assert!(registry.get(&embedded).is_some());
    }

    /// A preloaded document stands in for a download, which the durable cache
    /// records as written: the preloaded copy keeps the Core @context
    /// reference that the copy prepared for processing drops.
    #[test]
    fn a_preloaded_document_is_kept_as_written_beside_its_prepared_copy() {
        let registry = ContextRegistry::new();
        let url = IriBuf::new("https://forge.example.com/compound.jsonld".to_string()).expect("valid URL");
        let written = json!({ "@context": ["https://forge.example.com/terms.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()] });

        registry.register_preloaded(url.clone(), jsonld_syntax::Value::from_serde_json(written.clone())); // clone: the expectation compares against both

        let as_written = registry.preloaded_document(&url).expect("preloaded");
        pretty_assertions::assert_eq!(jsonld_syntax::Value::into_serde_json((*as_written).clone()), written);
        let prepared = registry.get(&url).expect("registered");
        pretty_assertions::assert_eq!(
            jsonld_syntax::Value::into_serde_json((*prepared.value).clone()),
            json!({ "@context": ["https://forge.example.com/terms.jsonld"] })
        );
    }
}
