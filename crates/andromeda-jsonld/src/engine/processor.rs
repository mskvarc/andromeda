//! Expanding and compacting a whole document.
//!
//! [`JsonLdEngine`] is the crate's public surface. Cloning it is cheap: the
//! loader and both caches are shared.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    document_context::apply_context,
    engine::{
        batch_context::RelativeUrlPolicy,
        builder::JsonLdEngineBuilder,
        context_url::{canonical_context_urls, context_urls_of, resolve_relative_context_urls, strip_context},
        options::{CompactOptions, ContextStripping},
        term_lookup::TermCache,
        term_map::UserTermTableCache,
    },
    error::JsonLdError,
    hit_counter::HitCounter,
    loader::http::HttpContextLoader,
};
use contextual::WithContext;
use jsonld::{
    JsonLdProcessor,
    RemoteContextReference,
    RemoteDocument,
    syntax::{Context as JsonLdContext, IntoJson, TryFromJson},
};
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
}

impl JsonLdEngine {
    /// Assemble an engine. Reached through [`JsonLdEngineBuilder`].
    pub(crate) fn assembled(loader: HttpContextLoader, term_cache: TermCache, hit_counter: Option<Arc<HitCounter>>) -> JsonLdEngine {
        JsonLdEngine {
            loader: Arc::new(loader),
            term_cache,
            hit_counter,
            user_term_tables: Arc::new(UserTermTableCache::default()),
        }
    }

    /// Start configuring an engine.
    pub fn builder() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }

    /// The shared access counter, when one is attached.
    ///
    /// The broker's startup path takes this so a periodic task can fold the
    /// accumulated deltas into the durable cache. Absent when no durable
    /// cache was attached, because there would be nothing to fold them into.
    pub fn hit_counter(&self) -> Option<Arc<HitCounter>> {
        self.hit_counter.clone() // clone: Arc ref-count bump
    }

    /// The context loader, for the batch term table.
    pub(crate) fn loader(&self) -> &HttpContextLoader {
        &self.loader
    }

    /// The parsed request contexts kept on this engine.
    pub(crate) fn user_term_tables(&self) -> &UserTermTableCache {
        &self.user_term_tables
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
        self.expand_prepared(document, context.core_policy(), RelativeUrlPolicy::MaybePresent).await
    }

    /// Expand a document that already carries its effective `@context`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::Expansion`] on an expansion failure, or a
    /// context-loading variant when a context did not resolve.
    pub(crate) async fn expand_prepared(
        &self,
        mut document: Value,
        core_policy: CoreContextPolicy,
        relative_url_policy: RelativeUrlPolicy,
    ) -> Result<Value, JsonLdError> {
        if relative_url_policy == RelativeUrlPolicy::MaybePresent {
            resolve_relative_context_urls(&mut document);
        }

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

    /// Compact a document against a context.
    ///
    /// IRIs become the short names the context declares. A compaction that
    /// fails because a request context redefines a protected core term is
    /// retried against the core context alone, which is the one failure a
    /// retry can fix.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::Compaction`] when compaction fails for any
    /// other reason, or a context-loading variant when a context did not
    /// resolve.
    pub async fn compact(&self, document: Value, context: &ContextSpec, options: CompactOptions) -> Result<Value, JsonLdError> {
        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
        let context_ref = context_reference(context)?;

        // Taken before the conversion consumes the document, and read only on
        // the failure path.
        let document_id = document
            .as_object()
            .and_then(|object| object.get("@id").or_else(|| object.get("id")))
            .and_then(Value::as_str)
            .map(str::to_owned);

        // One copy is unavoidable: compaction consumes the document, and the
        // protected-term retry needs the original back.
        let remote: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, jsonld_syntax::Value::from_serde_json(document.clone()));

        let compacted = match remote.compact_with(&mut vocabulary, context_ref, &*self.loader).await {
            Ok(compacted) => compacted,
            Err(error) if JsonLdError::is_protected_term_error(&error) => {
                tracing::warn!(
                    "Compaction failed with a protected-term redefinition: {}. Retrying with the core context alone; \
                     the request's own context is dropped, so short names may differ from what it declared.",
                    error
                );
                self.compact_with_core_context_only(document).await?
            }
            Err(error) => {
                tracing::error!(
                    error = %error,
                    document_id = document_id.as_deref().unwrap_or("<unknown>"),
                    "Compaction failed"
                );
                return Err(error.into());
            }
        };

        let mut compacted = jsonld_syntax::Value::into_serde_json(compacted);
        if options.strip_context == ContextStripping::Strip {
            strip_context(&mut compacted);
        }
        Ok(compacted)
    }

    /// The protected-term fallback: compact against the core context alone.
    async fn compact_with_core_context_only(&self, document: Value) -> Result<jsonld_syntax::Value, JsonLdError> {
        let core_ref = context_reference(&ContextSpec::core_only())?;
        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
        let remote: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, jsonld_syntax::Value::from_serde_json(document));
        remote.compact_with(&mut vocabulary, core_ref, &*self.loader).await.map_err(|error| {
            tracing::error!("Compaction failed even against the core context: {}", error);
            JsonLdError::from(error)
        })
    }

    /// Resolve a context URL to its payload through the full pipeline.
    ///
    /// # Errors
    ///
    /// Returns a context-loading variant when the URL does not resolve.
    pub async fn resolve_context(&self, url: &str) -> Result<Value, JsonLdError> {
        let value = self.loader.load_value(url).await?;
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
    pub async fn fetch_remote_context(&self, url: &str) -> Result<Value, JsonLdError> {
        let value = self.loader.fetch_remote_value(url).await.map_err(|error| JsonLdError::from(&error))?;
        Ok(jsonld_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
    }
}

/// The context reference the processor compacts against.
fn context_reference(context: &ContextSpec) -> Result<RemoteContextReference<IriIndex>, JsonLdError> {
    let jsonld_context = match context.to_context_value() {
        Some(value) => {
            let syntax_value = jsonld_syntax::Value::from_serde_json(value);
            JsonLdContext::try_from_json(&syntax_value).map_err(|error| JsonLdError::ContextInvalid {
                url: "inline".to_string(),
                message: error.to_string(),
            })?
        }
        None => JsonLdContext::default(),
    };
    Ok(RemoteContextReference::Loaded(RemoteDocument::new(None, None, jsonld_context)))
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::{NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
        engine::processor::JsonLdEngine,
    };
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
        let iri = offline_engine()
            .expand_type("http://example.com/Type", &ContextSpec::none())
            .await
            .expect("an IRI needs no context");

        pretty_assertions::assert_eq!(iri, "http://example.com/Type");
    }

    #[tokio::test]
    async fn a_core_context_type_term_expands_through_the_default_vocabulary() {
        let iri = offline_engine().expand_type("Vehicle", &ContextSpec::none()).await.expect("expansion");

        assert!(iri.contains("Vehicle"), "expected the term in the expanded IRI, got {iri}");
    }
}
