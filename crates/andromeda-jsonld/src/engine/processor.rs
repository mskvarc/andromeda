//! Expanding a whole document.
//!
//! [`JsonLdEngine`] is the crate's public surface. Cloning it is cheap: the
//! loader and every cache are shared. Compacting is
//! [`crate::engine::context_compactor`]'s.

use crate::{
    context_cache::DownloadedContext,
    context_graph::check_document_contexts,
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
    loader::{cache_lifetime::CacheLifetime, http::HttpContextLoader},
    presented_context::{PresentedContext, notification_body_context},
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
    /// Generated vocabulary IRIs validated once during engine construction.
    core_vocabulary: Arc<CoreVocabulary>,
}

impl JsonLdEngine {
    /// Assemble an engine. Reached through [`JsonLdEngineBuilder`].
    pub(crate) fn assembled(
        loader: HttpContextLoader,
        term_cache: TermCache,
        hit_counter: Option<Arc<HitCounter>>,
        core_vocabulary: CoreVocabulary,
    ) -> JsonLdEngine {
        JsonLdEngine {
            loader: Arc::new(loader),
            term_cache,
            hit_counter,
            user_term_tables: Arc::new(UserTermTableCache::default()),
            compaction_contexts: CompactionContextCache::new(),
            core_vocabulary: Arc::new(core_vocabulary),
        }
    }

    /// Start configuring an engine.
    #[must_use]
    pub fn builder() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
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

    /// How long this engine uses its copy of an `@context` the broker stores
    /// itself (clause 5.13.1 `"Hosted"` and `"ImplicitlyCreated"`) before
    /// reading the stored row again.
    ///
    /// The Serve operation advertises the same span as the `max-age` of such a
    /// document, so a remote party caching it is held to what this process is.
    #[must_use]
    pub fn hosted_context_revalidation(&self) -> CacheLifetime {
        self.loader.hosted_context_revalidation()
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
        resolve_relative_context_urls(&mut document, self.loader.hosted_contexts());
        check_document_contexts(&self.loader, &document).await?;

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

    /// The `@context` a response to a request that supplied `context` names
    /// (clauses 5.2.3, 4.4 and 5.5.7).
    ///
    /// Reads what the documents the request named leave in force last through
    /// the same loading pipeline processing reads them through; see
    /// [`PresentedContext`].
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] a named document cannot be loaded with.
    pub async fn presented_context(&self, context: &ContextSpec) -> Result<PresentedContext, JsonLdError> {
        PresentedContext::resolve(context, self.loader.as_ref()).await
    }

    /// The `@context` an `application/ld+json` or `application/geo+json`
    /// notification body names for a subscription's `jsonldContext`
    /// (clauses 5.8.1.4, 5.8.6 and 5.2.3).
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] the `jsonldContext` document cannot be
    /// loaded with.
    pub async fn notification_body_context(&self, jsonld_context: Option<&IriBuf>) -> Result<Value, JsonLdError> {
        notification_body_context(jsonld_context, self.loader.as_ref()).await
    }

    /// Resolve a context URL to its payload through the full pipeline.
    ///
    /// # Errors
    ///
    /// Returns a context-loading variant when the URL does not resolve.
    pub async fn resolve_context(&self, url: impl AsRef<str>) -> Result<Value, JsonLdError> {
        let url = IriBuf::new(url.as_ref().to_owned()).map_err(JsonLdError::ContextUrlNotIri)?;
        let value = self.loader.load_value(&url).await?;
        Ok(jsonld_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
    }

    /// Fetch a context from its source, bypassing every cache.
    ///
    /// Used when the caller needs to know the source is still reachable, as
    /// delete-with-reload does. The answer is the document as downloaded and
    /// the URL it was finally read from, which is what a "Cached" row stores.
    ///
    /// # Errors
    ///
    /// Returns a context-loading variant when the fetch fails.
    pub async fn fetch_remote_context(&self, url: &IriBuf) -> Result<DownloadedContext, JsonLdError> {
        self.loader.fetch_remote_value(url).await.map_err(|error| JsonLdError::from(&error))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::NGSI_LD_CORE_CONTEXT_V16_URL,
        engine::{expanded_input::ExpandedInput, options::CompactOptions, preloaded_engine::engine_preloading, processor::JsonLdEngine},
        loader::test_origin::{TestOrigin, ok, redirect},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use serde_json::{Value, json};

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

    /// The first of two contexts that both define `speed`.
    const REPEATED_A: &str = "https://example.org/repeated-a.jsonld";
    /// The second of two contexts that both define `speed`.
    const REPEATED_B: &str = "https://example.org/repeated-b.jsonld";

    /// An engine preloading two contexts that define `speed` differently.
    fn engine_with_two_speeds() -> JsonLdEngine {
        engine_preloading(&[
            (REPEATED_A.to_owned(), json!({ "@context": { "speed": "https://example.org/a#speed" } })),
            (REPEATED_B.to_owned(), json!({ "@context": { "speed": "https://example.org/b#speed" } })),
        ])
    }

    /// A vehicle carrying one `speed` Property.
    fn vehicle_with_speed() -> Value {
        json!({ "id": "urn:ngsi-ld:Vehicle:repeated", "type": "Vehicle", "speed": { "type": "Property", "value": 1 } })
    }

    /// Clause 5.5.7 processes the context the call provided, and JSON-LD 1.1
    /// API section 4.1.2 step 5.2.2 skips a repeated context only while a
    /// scoped context is validated: `[A, B, A]` processes A again after B, so
    /// a term both define expands to A's IRI.
    #[tokio::test]
    async fn a_context_repeated_in_the_request_leaves_its_last_occurrence_in_force() {
        let engine = engine_with_two_speeds();
        let context = ContextSpec::from_body(json!([REPEATED_A, REPEATED_B, REPEATED_A]));

        let expanded = engine.expand(vehicle_with_speed(), &context).await.expect("expansion succeeds");

        assert!(expanded[0].get("https://example.org/a#speed").is_some(), "{expanded}");
        assert!(expanded[0].get("https://example.org/b#speed").is_none(), "{expanded}");
    }

    /// The context a response presents for `[A, B, A]` keeps every entry the
    /// request named, repeat included, with the core context last (clause 4.4),
    /// and a client processing that presented context reads `speed` exactly
    /// as the broker did: as A's term.
    #[tokio::test]
    async fn a_repeated_context_is_presented_as_named_and_processed_under_its_last_occurrence() {
        let engine = engine_with_two_speeds();
        let context = ContextSpec::from_body(json!([REPEATED_A, REPEATED_B, REPEATED_A]));

        let presented = engine.presented_context(&context).await.expect("the context is presented").into_value();
        pretty_assertions::assert_eq!(presented, json!([REPEATED_A, REPEATED_B, REPEATED_A, NGSI_LD_CORE_CONTEXT_URL]));

        let expanded = engine
            .expand(vehicle_with_speed(), &ContextSpec::from_body(presented))
            .await
            .expect("expansion under the presented context succeeds");
        assert!(expanded[0].get("https://example.org/a#speed").is_some(), "{expanded}");
        assert!(expanded[0].get("https://example.org/b#speed").is_none(), "{expanded}");
    }

    /// JSON-LD 1.1 API section 4.1.2 step 5.2.6 processes a remote context
    /// against "the documentUrl of context document", its final URL: a
    /// context reached through a redirect finds the document its relative
    /// reference names beside where it was read from, which the requested
    /// directory does not hold.
    #[tokio::test]
    async fn a_relative_reference_in_a_redirected_context_is_processed_against_the_final_url() {
        let origin = TestOrigin::start().await;
        origin.stage("/moved.jsonld", redirect("/contexts/v2/wrapper.jsonld")).await;
        origin
            .stage("/contexts/v2/wrapper.jsonld", ok("application/ld+json", r#"{"@context": "terms.jsonld"}"#, &[]))
            .await;
        origin
            .stage(
                "/contexts/v2/terms.jsonld",
                ok("application/ld+json", r#"{"@context": {"speed": "https://example.org/v2#speed"}}"#, &[]),
            )
            .await;
        let engine = JsonLdEngine::builder().allow_remote(true).build().expect("engine builds");

        let expanded = engine
            .expand(vehicle_with_speed(), &ContextSpec::from_link(origin.url("/moved.jsonld")))
            .await
            .expect("expansion succeeds");

        assert!(expanded[0].get("https://example.org/v2#speed").is_some(), "{expanded}");
        pretty_assertions::assert_eq!(origin.requests("/terms.jsonld"), 0);
        pretty_assertions::assert_eq!(origin.requests("/contexts/v2/terms.jsonld"), 1);
    }

    #[tokio::test]
    async fn a_core_context_type_term_expands_through_the_default_vocabulary() {
        let engine = offline_engine();
        let context = engine.prepare_term_context(&ContextSpec::none());
        let iri = engine.expand_type("Vehicle", &context).await.expect("expansion");

        assert!(iri.as_str().contains("Vehicle"), "expected the term in the expanded IRI, got {iri}");
    }
}
