//! The contexts that resolve without a network.
//!
//! Holds the embedded core contexts and whatever the operator preloaded from
//! disk. Documents are normalised once, at registration (a redundant embedded
//! Core @context reference is dropped per [`crate::core_context_precedence`]),
//! so a lookup on the expansion path is a map read and nothing else.

use crate::{
    context_tail::ContextTail,
    engine::context_document::{ContextDocument, RelativeReferences},
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use dashmap::DashMap;
use iri_rs::IriBuf;
use std::sync::Arc;

/// How a registered document came to the registry.
///
/// Decides what the first resolution does: an `Embedded` document is compiled
/// into the broker (the Core @context per clause 4.4, the W3C Data Integrity
/// context), is never listed, served or stored, and leaves no durable cache
/// record; a `Preloaded` document is the operator's local stand-in for a
/// download, so the first resolution records it as a clause 5.13.1 "Cached"
/// entry, as written like every download.
#[derive(Debug, Clone)]
pub enum RegistrationSource {
    /// Compiled into the broker; resolution reads it and records nothing.
    Embedded,
    /// Preloaded from disk as a stand-in for a download; carries the document
    /// as written, which is what a "Cached" entry stores.
    Preloaded {
        /// The document exactly as the operator's file has it.
        as_written: Arc<JsonLiteral>,
    },
}

/// One registered document: the copy the JSON-LD processor reads, and what the
/// document as written leaves in force last.
#[derive(Debug, Clone)]
pub struct RegisteredContext {
    /// The document with its embedded Core @context references removed.
    pub document: Arc<ContextDocument>,
    /// What the document as written leaves in force last (clause 4.4).
    pub tail: ContextTail,
    /// How the document came to the registry.
    pub source: RegistrationSource,
}

/// A thread-safe map from context URL to a parsed context document.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    /// The registered documents, shared by every clone of the registry.
    contexts: Arc<DashMap<IriBuf, RegisteredContext>>,
}

impl ContextRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> ContextRegistry {
        ContextRegistry::default()
    }

    /// Register a context document under a URL.
    ///
    /// A redundant embedded Core @context reference is dropped at registration,
    /// so the broker's authoritative Core --
    /// appended last per clause 4.4 -- stays the single occurrence and a
    /// registered user context that redefines a core term resolves to the
    /// Core's definition rather than failing.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError::ProcessorText`] when the JSON-LD processor's
    /// reader refuses the admitted document.
    pub fn register(&self, url: IriBuf, written: &JsonLiteral) -> Result<(), JsonLiteralError> {
        self.register_with_source(url, written, RegistrationSource::Embedded)
    }

    /// Register a context document the operator preloaded from disk.
    ///
    /// Behaves like [`Self::register`] but marks the entry
    /// [`RegistrationSource::Preloaded`] carrying the document as written, so
    /// the resolver records a clause 5.13.1 "Cached" copy on first resolution.
    /// The engine builder refuses to preload a core context URL, so this never
    /// marks an embedded context preloaded.
    ///
    /// # Errors
    ///
    /// As [`Self::register`].
    pub fn register_preloaded(&self, url: IriBuf, written: JsonLiteral) -> Result<(), JsonLiteralError> {
        let as_written = Arc::new(written);
        self.register_with_source(
            url,
            &as_written,
            RegistrationSource::Preloaded {
                as_written: Arc::clone(&as_written), // clone: the source owns the document while the borrow above only parsed it.
            },
        )
    }

    /// The shared body of [`Self::register`] and [`Self::register_preloaded`].
    ///
    /// # Errors
    ///
    /// As [`Self::register`].
    fn register_with_source(&self, url: IriBuf, written: &JsonLiteral, source: RegistrationSource) -> Result<(), JsonLiteralError> {
        let prepared = ContextDocument::prepared(&url, written, RelativeReferences::AsWritten)?;
        self.contexts.insert(
            url,
            RegisteredContext {
                document: Arc::new(prepared.document),
                tail: prepared.tail,
                source,
            },
        );
        Ok(())
    }

    /// The document registered for a URL, if there is one.
    #[must_use]
    pub fn get(&self, url: &IriBuf) -> Option<RegisteredContext> {
        self.contexts.get(url).map(|entry| entry.value().clone()) // clone: an `Arc` bump and the tail, handed out while the map keeps its entry
    }
}

impl RegisteredContext {
    /// The document as the operator wrote it, when the operator preloaded it;
    /// `None` for a document the broker embeds.
    #[must_use]
    pub const fn preloaded_written(&self) -> Option<&Arc<JsonLiteral>> {
        match &self.source {
            RegistrationSource::Embedded => None,
            RegistrationSource::Preloaded { as_written } => Some(as_written),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_registry::{ContextRegistry, RegistrationSource},
        context_tail::ContextTail,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use jstrict::Print;
    use serde_json::{Value, json};

    fn literal(value: &Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible document")
    }

    /// The copy of the document registered under `url` the processor reads.
    fn prepared(registry: &ContextRegistry, url: &IriBuf) -> Value {
        let registered = registry.get(url).expect("registered");
        serde_json::from_str(&registered.document.json().compact_print().to_string()).expect("JSON")
    }

    #[test]
    fn a_registered_user_context_drops_its_redundant_core_import() {
        let registry = ContextRegistry::new();
        let url = IriBuf::new("https://example.com/user.jsonld".to_string()).expect("valid URL");
        registry
            .register(
                url.clone(), // clone: the lookup below names the same URL
                &literal(&json!({ "@context": [NGSI_LD_CORE_CONTEXT_URL, { "speed": "https://example.com/speed" }] })),
            )
            .expect("registered");

        pretty_assertions::assert_eq!(prepared(&registry, &url), json!({ "@context": [{ "speed": "https://example.com/speed" }] }));
    }

    /// Clause 4.4 and Annex C.8: the registry keeps what the document as
    /// written left in force last, although the copy it hands the processor
    /// does not name the Core.
    #[test]
    fn a_registered_document_keeps_what_it_left_in_force_last() {
        let registry = ContextRegistry::new();
        let url = IriBuf::new("https://example.com/compound.jsonld".to_string()).expect("valid URL");
        registry
            .register(
                url.clone(), // clone: the registry keys the entry on the URL the lookup below names
                &literal(&json!({ "@context": ["https://example.com/terms.jsonld", NGSI_LD_CORE_CONTEXT_URL] })),
            )
            .expect("registered");

        pretty_assertions::assert_eq!(registry.get(&url).map(|registered| registered.tail), Some(ContextTail::CurrentCore));
    }

    #[test]
    fn an_unregistered_url_resolves_to_nothing() {
        let url = IriBuf::new("https://example.com/absent.jsonld".to_string()).expect("valid URL");
        assert!(ContextRegistry::new().get(&url).is_none());
    }

    /// Clause 5.13.1: a document the operator preloaded is a local copy of one
    /// the broker would otherwise download, so the entry says where it came
    /// from; an embedded one says `Embedded`, and the resolver leaves it out
    /// of the Cached store on that basis.
    #[test]
    fn a_registration_names_whether_the_operator_preloaded_it() {
        let registry = ContextRegistry::new();
        let preloaded = IriBuf::new("https://forge.example.com/suite.jsonld".to_string()).expect("valid URL");
        let embedded = IriBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_string()).expect("valid URL");

        registry
            .register_preloaded(preloaded.clone(), literal(&json!({ "@context": {} }))) // clone: looked up below
            .expect("registered");
        registry
            .register(embedded.clone(), &literal(&json!({ "@context": {} }))) // clone: looked up below
            .expect("registered");

        assert!(registry.get(&preloaded).is_some_and(|registered| registered.preloaded_written().is_some()));
        assert!(registry.get(&embedded).is_some_and(|registered| registered.preloaded_written().is_none()));
    }

    /// A preloaded document stands in for a download, which the durable cache
    /// records as written: the preloaded copy keeps the Core @context
    /// reference that the copy prepared for processing drops.
    #[test]
    fn a_preloaded_document_is_kept_as_written_beside_its_prepared_copy() {
        let registry = ContextRegistry::new();
        let url = IriBuf::new("https://forge.example.com/compound.jsonld".to_string()).expect("valid URL");
        let written = json!({ "@context": ["https://forge.example.com/terms.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()] });

        registry.register_preloaded(url.clone(), literal(&written)).expect("registered"); // clone: looked up below

        let registered = registry.get(&url).expect("registered");
        assert!(matches!(registered.source, RegistrationSource::Preloaded { .. }));
        let as_written = registered.preloaded_written().expect("preloaded");
        pretty_assertions::assert_eq!(serde_json::from_str::<Value>(as_written.text()).expect("JSON"), written);
        pretty_assertions::assert_eq!(prepared(&registry, &url), json!({ "@context": ["https://forge.example.com/terms.jsonld"] }));
    }
}
