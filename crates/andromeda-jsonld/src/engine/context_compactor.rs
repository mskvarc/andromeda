//! Compacting every document of one operation against one `@context`.
//!
//! A query page, a temporal result set and a notification batch all compact
//! many documents against the same request context. [`ContextCompactor`]
//! processes that context the first time a document needs it and compacts
//! every later document against the same result, so the per-document cost is
//! the compaction walk and nothing else.

use crate::{
    context_spec::ContextSpec,
    engine::{
        compaction_context::{CompactionContext, CompactionContextError, ContextEmbedding},
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
    error::JsonLdError,
};
use iri_rs::IriBuf;
use jsonld::{CompactError, ExpandedDocument, JsonLdProcessor, Options, RemoteDocument};
use rdfx::{BlankIdBuf, vocabulary::no_vocabulary_mut};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::OnceCell;

/// The warning a protected-term redefinition logs before its fallback.
const PROTECTED_TERM_FALLBACK: &str = "Compaction failed with a protected-term redefinition. Retrying with the core context alone; the request's own \
                                       context is dropped, so short names may differ from what it declared.";

/// Compacts documents against one request `@context`, processing it once.
///
/// The context is processed lazily, when the first document is compacted, so
/// an operation that ends up compacting nothing never resolves it and cannot
/// fail on a context it did not need.
#[derive(Debug)]
pub struct ContextCompactor<'a> {
    /// The engine whose loader resolves the context.
    engine: &'a JsonLdEngine,
    /// The context documents are compacted against.
    context: &'a ContextSpec,
    /// The processed context, once the first document needed it.
    prepared: OnceCell<Arc<CompactionContext>>,
}

impl JsonLdEngine {
    /// A compactor for every document one operation compacts against
    /// `context`.
    #[must_use]
    pub fn compactor<'a>(&'a self, context: &'a ContextSpec) -> ContextCompactor<'a> {
        ContextCompactor {
            engine: self,
            context,
            prepared: OnceCell::new(),
        }
    }
}

impl<'a> ContextCompactor<'a> {
    /// The engine this compactor resolves contexts through.
    #[must_use]
    pub const fn engine(&self) -> &'a JsonLdEngine {
        self.engine
    }

    /// The context documents are compacted against.
    #[must_use]
    pub const fn context(&self) -> &'a ContextSpec {
        self.context
    }

    /// Compact an expanded document.
    ///
    /// IRIs become the short names the context declares. A compaction that
    /// fails because the context redefines a protected core term is retried
    /// against the core context alone, which is the one failure a retry can
    /// fix. The redefinition is found either while the context is processed,
    /// which settles the fallback for every document of the operation, or while
    /// compaction applies a type-scoped context only some documents select,
    /// which settles it for that document alone.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::Compaction`] when compaction fails for any other
    /// reason, [`JsonLdError::InlineContextInvalid`] when the resolved
    /// `@context` is not a JSON-LD context, or a context-loading variant when a
    /// context did not resolve.
    pub async fn compact(&self, document: Value, options: CompactOptions) -> Result<Value, JsonLdError> {
        let expanded = self.expand(document).await?;
        let prepared = self.prepared().await?;
        let embedding = match options.strip_context {
            ContextStripping::Strip => ContextEmbedding::Omit,
            ContextStripping::Keep => ContextEmbedding::Embed,
        };

        let compacted = match prepared.compact(&expanded, self.engine.loader(), embedding).await {
            Ok(compacted) => compacted,
            Err(error) if JsonLdError::is_protected_term_error(&error) => {
                tracing::warn!(error = %error, "{PROTECTED_TERM_FALLBACK}");
                let core = self.core_context().await?;
                core.compact(&expanded, self.engine.loader(), embedding).await.map_err(|error| {
                    tracing::error!("Compaction failed even against the core context: {}", error);
                    JsonLdError::from(*error)
                })?
            }
            Err(error) => {
                tracing::error!(error = %error, document_id = document_id(&expanded), "Compaction failed");
                return Err(JsonLdError::from(*error));
            }
        };
        Ok(jsonld_syntax::Value::into_serde_json(compacted))
    }

    /// Read an expanded document into the processor's own form.
    ///
    /// The document is already expanded, but it is taken through the
    /// expansion algorithm rather than read verbatim: expansion also
    /// normalizes it (a lone value becomes an array, a member whose key is not
    /// an absolute IRI is dropped), and the documents handed here are built by
    /// several producers that rely on that.
    async fn expand(&self, document: Value) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, JsonLdError> {
        let remote: RemoteDocument<IriBuf> = RemoteDocument::new(None, None, jsonld_syntax::Value::from_serde_json(document));
        JsonLdProcessor::expand_full(&remote, no_vocabulary_mut(), self.engine.loader(), Options::default(), ())
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Compaction failed");
                JsonLdError::from(CompactError::Expand(error))
            })
    }

    /// The processed request context, processing it on first use.
    ///
    /// A failed attempt is not remembered, so the next document tries again,
    /// exactly as it would have had it been the first.
    async fn prepared(&self) -> Result<&CompactionContext, JsonLdError> {
        self.prepared.get_or_try_init(|| self.prepare()).await.map(Arc::as_ref)
    }

    /// Process the request context, or the core context alone when the request
    /// context redefines a protected core term.
    async fn prepare(&self) -> Result<Arc<CompactionContext>, JsonLdError> {
        match self.engine.compaction_contexts().get_or_prepare(self.context, self.engine.loader()).await {
            Ok(prepared) => Ok(prepared),
            Err(CompactionContextError::Processing(error)) if JsonLdError::is_protected_term_error(&error) => {
                tracing::warn!(error = %error, "{PROTECTED_TERM_FALLBACK}");
                self.core_context().await
            }
            Err(CompactionContextError::Processing(error)) => {
                tracing::error!(error = %error, "Compaction failed");
                Err(JsonLdError::from(*error))
            }
            Err(CompactionContextError::Invalid(error)) => Err(error),
        }
    }

    /// The core context alone, which a protected-term redefinition falls back
    /// to.
    async fn core_context(&self) -> Result<Arc<CompactionContext>, JsonLdError> {
        self.engine
            .compaction_contexts()
            .get_or_prepare(&ContextSpec::core_only(), self.engine.loader())
            .await
            .map_err(|error| {
                tracing::error!("Compaction failed even against the core context: {}", error);
                match error {
                    CompactionContextError::Invalid(error) => error,
                    CompactionContextError::Processing(error) => JsonLdError::from(*error),
                }
            })
    }
}

/// The identifier of a document's first node, for a failure diagnostic.
fn document_id(document: &ExpandedDocument<IriBuf, BlankIdBuf>) -> &str {
    document.iter().find_map(|object| object.id()).map_or("<unknown>", |id| id.as_str())
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::{ContextSpec, CoreContextPolicy},
        core_context::NGSI_LD_CORE_CONTEXT_V16_URL,
        engine::{
            options::{CompactOptions, ContextStripping},
            processor::JsonLdEngine,
        },
        error::JsonLdError,
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriRefBuf;
    use jsonld::{
        JsonLdProcessor,
        RemoteContextReference,
        RemoteDocument,
        syntax::{Context as JsonLdContext, TryFromJson},
    };
    use rdfx::vocabulary::{BlankIdIndex, IndexVocabulary, IriIndex};
    use serde_json::{Value, json};
    use std::sync::Arc;

    const STRIP: CompactOptions = CompactOptions {
        strip_context: ContextStripping::Strip,
    };
    const KEEP: CompactOptions = CompactOptions {
        strip_context: ContextStripping::Keep,
    };

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// A normalized entity exercising every attribute family the core context
    /// types differently.
    fn normalized_entity() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:A4567",
            "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes", "observedAt": "2020-08-01T12:03:00Z"},
            "speed": [
                {"type": "Property", "value": 55, "datasetId": "urn:ngsi-ld:Dataset:gps"},
                {"type": "Property", "value": 54.5, "unitCode": "KMH", "datasetId": "urn:ngsi-ld:Dataset:obd"}
            ],
            "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:Downtown1"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "config": {"type": "JsonProperty", "json": {"nested": {"n": 0.5}}},
            "readings": {"type": "ListProperty", "valueList": [1, 2, 3]},
            "category": {"type": "VocabProperty", "vocab": "commercial"}
        })
    }

    /// The request context of clause C.2.4, carried in the body so the test
    /// needs no network.
    fn example_body_context() -> ContextSpec {
        ContextSpec::none().with_body_context(Some(json!({
            "Vehicle": "http://example.org/vehicle/Vehicle",
            "brandName": "http://example.org/vehicle/brandName",
            "speed": "http://example.org/vehicle/speed",
            "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"}
        })))
    }

    /// Compaction as the engine performed it before contexts were prepared
    /// once: the document is re-expanded and compacted against a context
    /// processed for it alone, with the `@context` removed afterwards when it
    /// is not kept. The prepared path must match it byte for byte.
    async fn per_document_compaction(engine: &JsonLdEngine, document: Value, spec: &ContextSpec, options: CompactOptions) -> Value {
        let context = match spec.to_context_value() {
            Some(value) => JsonLdContext::try_from_json(&jsonld_syntax::Value::from_serde_json(value)).expect("a JSON-LD context"),
            None => JsonLdContext::default(),
        };
        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
        let remote: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, jsonld_syntax::Value::from_serde_json(document));
        let compacted = remote
            .compact_with(
                &mut vocabulary,
                RemoteContextReference::Loaded(RemoteDocument::new(None, None, context)),
                engine.loader(),
            )
            .await
            .expect("per-document compaction succeeds");
        let mut compacted = jsonld_syntax::Value::into_serde_json(compacted);
        if options.strip_context == ContextStripping::Strip
            && let Some(object) = compacted.as_object_mut()
        {
            object.remove("@context");
        }
        compacted
    }

    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    #[tokio::test]
    async fn prepared_compaction_matches_per_document_compaction_byte_for_byte() {
        let engine = offline_engine();
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");
        let specs = [ContextSpec::none(), ContextSpec::core_only(), example_body_context()];

        for spec in &specs {
            for options in [STRIP, KEEP] {
                let prepared = engine
                    .compactor(spec)
                    .compact(expanded.clone(), options)
                    .await
                    .expect("prepared compaction succeeds");
                let reference = per_document_compaction(&engine, expanded.clone(), spec, options).await;

                pretty_assertions::assert_eq!(
                    serde_json::to_string(&prepared).expect("prepared JSON"),
                    serde_json::to_string(&reference).expect("reference JSON"),
                    "spec {spec:?} with {options:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn every_document_of_one_compactor_matches_its_own_per_document_compaction() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let first = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");
        let second = engine
            .expand(
                json!({"id": "urn:ngsi-ld:Vehicle:B", "type": "Vehicle", "speed": {"type": "Property", "value": 12}}),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");

        for document in [first, second] {
            let prepared = compactor.compact(document.clone(), STRIP).await.expect("prepared compaction succeeds");
            pretty_assertions::assert_eq!(prepared, per_document_compaction(&engine, document, &spec, STRIP).await);
        }
    }

    #[tokio::test]
    async fn a_kept_context_is_the_only_member_a_stripped_document_lacks() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        let mut kept = compactor.compact(expanded.clone(), KEEP).await.expect("compaction succeeds");
        let stripped = compactor.compact(expanded, STRIP).await.expect("compaction succeeds");
        let context = kept.as_object_mut().and_then(|object| object.remove("@context"));

        pretty_assertions::assert_eq!(context, spec.to_context_value());
        pretty_assertions::assert_eq!(kept, stripped);
    }

    #[tokio::test]
    async fn one_compactor_processes_its_context_once_for_every_document() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        compactor.compact(expanded.clone(), STRIP).await.expect("first compaction succeeds");
        let first = Arc::clone(compactor.prepared.get().expect("the first document prepared the context"));
        compactor.compact(expanded, STRIP).await.expect("second compaction succeeds");
        let second = compactor.prepared.get().expect("the context stays prepared");

        assert!(Arc::ptr_eq(&first, second), "the second document must reuse the context the first one prepared");
    }

    #[tokio::test]
    async fn an_embedded_context_is_processed_once_across_compactors() {
        let engine = offline_engine();
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");
        let none = ContextSpec::none();
        let versioned = ContextSpec::from_links(vec![iri_ref(NGSI_LD_CORE_CONTEXT_V16_URL.as_str())]);
        let first = engine.compactor(&none);
        let second = engine.compactor(&versioned);

        first.compact(expanded.clone(), STRIP).await.expect("first compaction succeeds");
        second.compact(expanded, STRIP).await.expect("second compaction succeeds");

        assert!(Arc::ptr_eq(first.prepared.get().expect("prepared"), second.prepared.get().expect("prepared")));
        pretty_assertions::assert_eq!(engine.compaction_contexts().len().await, 1);
    }

    #[tokio::test]
    async fn a_body_context_is_processed_per_compactor_and_never_kept_on_the_engine() {
        let engine = offline_engine();
        let spec = example_body_context();
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        engine.compactor(&spec).compact(expanded, STRIP).await.expect("compaction succeeds");

        pretty_assertions::assert_eq!(engine.compaction_contexts().len().await, 0);
    }

    #[tokio::test]
    async fn a_compactor_that_compacts_nothing_never_resolves_its_context() {
        let engine = offline_engine();
        let spec = ContextSpec::from_links(vec![iri_ref("https://example.com/unreachable.jsonld")]);
        let compactor = engine.compactor(&spec);

        assert!(compactor.prepared.get().is_none());
    }

    #[tokio::test]
    async fn an_unresolvable_context_fails_every_document_and_is_never_remembered() {
        let engine = offline_engine();
        let spec = ContextSpec::from_links(vec![iri_ref("https://example.com/unreachable.jsonld")]);
        let compactor = engine.compactor(&spec);
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        let first = compactor.compact(expanded.clone(), STRIP).await;
        let second = compactor.compact(expanded, STRIP).await;

        assert!(first.as_ref().is_err_and(JsonLdError::is_context_not_available), "got {first:?}");
        assert!(second.as_ref().is_err_and(JsonLdError::is_context_not_available), "got {second:?}");
        assert!(compactor.prepared.get().is_none());
    }

    /// A context that redefines a protected core term after the core context
    /// fails while it is processed, so every document of the operation is
    /// compacted against the core context alone.
    #[tokio::test]
    async fn a_context_redefining_a_protected_term_compacts_every_document_against_the_core_context() {
        let engine = offline_engine();
        let spec = ContextSpec::none()
            .with_body_context(Some(json!([NGSI_LD_CORE_CONTEXT_URL.as_str(), {"type": "http://example.org/redefined/type"}])))
            .with_core_policy(CoreContextPolicy::Omit);
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        let compacted = engine.compactor(&spec).compact(expanded.clone(), STRIP).await.expect("the fallback compacts");
        let core_only = per_document_compaction(&engine, expanded, &ContextSpec::core_only(), STRIP).await;

        pretty_assertions::assert_eq!(compacted, core_only);
    }

    /// A type-scoped context is processed only for a node of that type, so a
    /// protected-term redefinition inside one falls back for that document
    /// alone; a document of another type still gets the request's own terms.
    #[tokio::test]
    async fn a_type_scoped_protected_term_redefinition_falls_back_for_that_document_alone() {
        let engine = offline_engine();
        let spec = ContextSpec::none().with_body_context(Some(json!({
            "Vehicle": {"@id": "http://example.org/vehicle/Vehicle", "@context": {"type": "http://example.org/redefined/type"}},
            "speed": "http://example.org/vehicle/speed"
        })));
        let compactor = engine.compactor(&spec);
        let vehicle = engine
            .expand(
                json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "http://example.org/vehicle/Vehicle", "http://example.org/vehicle/speed": {"type": "Property", "value": 1}}),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");
        let bicycle = engine
            .expand(
                json!({"id": "urn:ngsi-ld:Bicycle:1", "type": "http://example.org/vehicle/Bicycle", "http://example.org/vehicle/speed": {"type": "Property", "value": 2}}),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");

        let vehicle_compacted = compactor.compact(vehicle.clone(), STRIP).await.expect("the fallback compacts");
        let bicycle_compacted = compactor.compact(bicycle, STRIP).await.expect("compaction succeeds");

        pretty_assertions::assert_eq!(
            vehicle_compacted,
            per_document_compaction(&engine, vehicle, &ContextSpec::core_only(), STRIP).await
        );
        assert!(
            bicycle_compacted.get("speed").is_some(),
            "the request context still applies: {bicycle_compacted}"
        );
    }
}
