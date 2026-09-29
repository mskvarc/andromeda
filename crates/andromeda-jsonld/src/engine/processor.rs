//! Expanding a whole document.
//!
//! [`JsonLdEngine`] is the crate's public surface. Cloning it is cheap: the
//! loader and every cache are shared. Compacting is
//! [`crate::engine::context_compactor`]'s.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    document_context::apply_context,
    engine::{
        builder::JsonLdEngineBuilder,
        compaction_context_cache::CompactionContextCache,
        context_url::{canonical_context_urls, context_urls_of, resolve_relative_context_urls},
        term_lookup::TermCache,
        term_map::{user_term::UserTermTableCache, vocabulary::CoreVocabulary},
    },
    error::JsonLdError,
    hit_counter::HitCounter,
    loader::http::HttpContextLoader,
};
use contextual::WithContext;
use iri_rs::IriBuf;
use jsonld::{JsonLdProcessor, RemoteDocument, syntax::IntoJson};
use rdfx::vocabulary::{BlankIdIndex, IndexVocabulary, IriIndex};
use serde_json::Value;
use std::sync::Arc;

/// The JSON-LD processor, bound to this broker's context resolution.
#[derive(Clone, Debug)]
pub struct JsonLdEngine {
    /// Where contexts come from.
    loader: Arc<HttpContextLoader>,
    /// Expanded single terms, keyed by term and context.
    term_cache: TermCache,
    /// Clause 5.13.3.5 access counts, when a durable cache is attached.
    hit_counter: Option<Arc<HitCounter>>,
    /// Parsed request contexts, kept per URL for the life of the engine.
    user_term_tables: Arc<UserTermTableCache>,
    /// Processed compaction contexts built only from embedded documents.
    compaction_contexts: CompactionContextCache,
    /// The released core context a versionless reference expands against
    /// (clause 4.4). The fastpath term table is derived from this version,
    /// so two engines built with different pins expand a versionless request
    /// differently, and this is where that decision lives rather than in a
    /// process-global.
    default_core_url: IriBuf,
    /// Generated vocabulary IRIs validated once during engine construction.
    core_vocabulary: Arc<CoreVocabulary>,
}

impl JsonLdEngine {
    /// Assemble an engine. Reached through [`JsonLdEngineBuilder`].
    pub(crate) fn assembled(
        loader: HttpContextLoader,
        term_cache: TermCache,
        hit_counter: Option<Arc<HitCounter>>,
        default_core_url: IriBuf,
        core_vocabulary: CoreVocabulary,
    ) -> JsonLdEngine {
        JsonLdEngine {
            loader: Arc::new(loader),
            term_cache,
            hit_counter,
            user_term_tables: Arc::new(UserTermTableCache::default()),
            compaction_contexts: CompactionContextCache::new(),
            default_core_url,
            core_vocabulary: Arc::new(core_vocabulary),
        }
    }

    /// Start configuring an engine.
    #[must_use]
    pub fn builder() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }

    /// The released core context a versionless reference expands against.
    ///
    /// The version this deployment pinned (clause 4.4); the fastpath term
    /// table is built from it. Defaults to the versionless core context when no
    /// pin was configured.
    #[must_use]
    pub const fn default_core_url(&self) -> &IriBuf {
        &self.default_core_url
    }

    /// The generated vocabulary after construction-time IRI validation.
    pub(crate) fn core_vocabulary(&self) -> &CoreVocabulary {
        &self.core_vocabulary
    }

    /// The shared access counter, when one is attached.
    ///
    /// The broker's startup path takes this so a periodic task can fold the
    /// accumulated deltas into the durable cache. Absent when no durable
    /// cache was attached, because there would be nothing to fold them into.
    #[must_use]
    pub fn hit_counter(&self) -> Option<Arc<HitCounter>> {
        self.hit_counter.clone() // clone: Arc ref-count bump
    }

    /// The context loader, for the batch term table.
    pub(crate) fn loader(&self) -> &HttpContextLoader {
        &self.loader
    }

    /// The processed compaction contexts kept on this engine.
    pub(crate) const fn compaction_contexts(&self) -> &CompactionContextCache {
        &self.compaction_contexts
    }

    /// The parsed request contexts kept on this engine.
    pub(crate) fn user_term_tables(&self) -> &UserTermTableCache {
        &self.user_term_tables
    }

    /// Invalidate every in-process cache entry for a context URL.
    ///
    /// Drops the URL from the moka document cache and from the parsed term
    /// tables, so a context that was just deleted or reloaded is re-resolved on
    /// next use rather than served from memory. The durable L2 cache is the
    /// caller's to update (a delete removes its row, a reload replaces its
    /// payload).
    pub async fn invalidate_context(&self, url: &IriBuf) {
        self.loader.invalidate(url).await;
        self.user_term_tables.remove(url);
    }

    /// The single-term expansion cache.
    pub(crate) const fn term_cache(&self) -> &TermCache {
        &self.term_cache
    }

    /// Expand a document to its canonical form.
    ///
    /// Short names become IRIs and the result is always an array of node
    /// objects.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::Expansion`] on an expansion failure, or a
    /// context-loading variant when a context did not resolve.
    pub async fn expand(&self, document: Value, context: &ContextSpec) -> Result<Value, JsonLdError> {
        let document = apply_context(document, context)?;
        self.expand_prepared(document, context.core_policy()).await
    }

    /// Expand a document that already carries its effective `@context`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::Expansion`] on an expansion failure, or a
    /// context-loading variant when a context did not resolve.
    pub(crate) async fn expand_prepared(&self, mut document: Value, core_policy: CoreContextPolicy) -> Result<Value, JsonLdError> {
        resolve_relative_context_urls(&mut document);

        // The diagnostic needs the document's `@context` and the conversion
        // below consumes the document, so the context alone is kept — the
        // happy path pays one shallow copy of it and nothing else.
        let diagnostic_context = document.get("@context").cloned();
        let remote: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, jsonld_syntax::Value::from_serde_json(document));
        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();

        let expanded = match remote.expand_with(&mut vocabulary, &*self.loader).await {
            Ok(expanded) => expanded,
            Err(error) => {
                let context_urls = diagnostic_context.as_ref().map(context_urls_of).unwrap_or_default();
                tracing::warn!(
                    "JSON-LD expansion failed. core_policy={:?}; applied_context={:?}; context_urls={:?}; canonical_context_urls={:?}; error={:?}",
                    core_policy,
                    diagnostic_context,
                    context_urls,
                    canonical_context_urls(&context_urls),
                    error
                );
                return Err(error.into());
            }
        };

        Ok(jsonld_syntax::Value::into_serde_json(
            expanded.into_iter().collect::<Vec<_>>().into_with(&vocabulary).into_json(),
        ))
    }

    /// Resolve a context URL to its payload through the full pipeline.
    ///
    /// # Errors
    ///
    /// Returns a context-loading variant when the URL does not resolve.
    pub async fn resolve_context(&self, url: impl AsRef<str>) -> Result<Value, JsonLdError> {
        let url = IriBuf::new(url.as_ref().to_owned()).map_err(|error| JsonLdError::invalid_iri(format!("context URL is not an IRI: {}", error.0)))?;
        let value = self.loader.load_value(&url).await?;
        Ok(jsonld_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
    }

    /// Fetch a context from its source, bypassing every cache.
    ///
    /// Used when the caller needs to know the source is still reachable, as
    /// delete-with-reload does.
    ///
    /// # Errors
    ///
    /// Returns a context-loading variant when the fetch fails.
    pub async fn fetch_remote_context(&self, url: &IriBuf) -> Result<Value, JsonLdError> {
        let value = self.loader.fetch_remote_value(url).await.map_err(|error| JsonLdError::from(&error))?;
        Ok(jsonld_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::NGSI_LD_CORE_CONTEXT_V16_URL,
        engine::{expanded_input::ExpandedInput, options::CompactOptions, processor::JsonLdEngine},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use serde_json::json;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[tokio::test]
    async fn expanding_an_entity_yields_an_array_whose_first_node_carries_the_identifier() {
        let expanded = offline_engine()
            .expand(
                json!({ "@context": NGSI_LD_CORE_CONTEXT_URL, "id": "urn:ngsi-ld:Vehicle:001", "type": "Vehicle" }),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");

        assert!(expanded.is_array());
        pretty_assertions::assert_eq!(expanded[0].get("@id"), Some(&json!("urn:ngsi-ld:Vehicle:001")));
    }

    #[tokio::test]
    async fn an_array_document_expands_to_an_array() {
        let expanded = offline_engine()
            .expand(
                json!([{ "@context": NGSI_LD_CORE_CONTEXT_URL, "id": "urn:ngsi-ld:Vehicle:001", "type": "Vehicle" }]),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");

        assert!(expanded.is_array());
    }

    #[tokio::test]
    async fn standards_expansion_and_compaction_preserve_exact_value_list_and_json_numbers() {
        let document = serde_json::from_str(
            r#"{"id":"urn:ngsi-ld:Device:exact","type":"Device","reading":{"type":"Property","value":0.100000000000000005},"readings":{"type":"ListProperty","valueList":[0.100000000000000005]},"config":{"type":"JsonProperty","json":{"n":0.100000000000000005}}}"#,
        )
        .expect("exact JSON");
        let engine = offline_engine();
        let context = ContextSpec::none();
        let expanded = engine.expand(document, &context).await.expect("expansion succeeds");
        let expanded_json = serde_json::to_string(&expanded).expect("expanded JSON");
        pretty_assertions::assert_eq!(expanded_json.matches("0.100000000000000005").count(), 3);
        assert!(!expanded_json.contains("$serde_json::private::Number"), "{expanded_json}");

        let compacted = engine
            .compactor(&context)
            .compact(ExpandedInput::Assembled(expanded), CompactOptions::default())
            .await
            .expect("compaction succeeds");
        let compacted_json = serde_json::to_string(&compacted).expect("compacted JSON");
        pretty_assertions::assert_eq!(compacted_json.matches("0.100000000000000005").count(), 3);
        assert!(!compacted_json.contains("$serde_json::private::Number"), "{compacted_json}");
    }

    #[tokio::test]
    async fn every_released_core_context_version_resolves_from_the_embedded_copies() {
        let expanded = offline_engine()
            .expand(
                json!({ "@context": NGSI_LD_CORE_CONTEXT_V16_URL, "id": "urn:ngsi-ld:Vehicle:core-v16", "type": "Vehicle" }),
                &ContextSpec::none(),
            )
            .await;

        assert!(expanded.is_ok(), "a released core context must resolve offline: {:?}", expanded.err());
    }

    #[tokio::test]
    async fn a_term_that_is_already_an_iri_expands_to_itself() {
        let engine = offline_engine();
        let context = engine.prepare_term_context(&ContextSpec::none());
        let iri = engine.expand_type("http://example.com/Type", &context).await.expect("an IRI needs no context");

        pretty_assertions::assert_eq!(iri.as_str(), "http://example.com/Type");
    }

    #[tokio::test]
    async fn a_core_context_type_term_expands_through_the_default_vocabulary() {
        let engine = offline_engine();
        let context = engine.prepare_term_context(&ContextSpec::none());
        let iri = engine.expand_type("Vehicle", &context).await.expect("expansion");

        assert!(iri.as_str().contains("Vehicle"), "expected the term in the expanded IRI, got {iri}");
    }
}
