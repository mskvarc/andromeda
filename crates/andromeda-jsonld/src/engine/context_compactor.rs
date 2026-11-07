//! Compacting every document of one operation against one `@context`.
//!
//! A query page, a temporal result set and a notification batch all compact
//! many documents against the same request context. [`ContextCompactor`]
//! processes that context the first time a document needs it and compacts
//! every later document against the same result, so the per-document cost is
//! the compaction walk and nothing else.

use crate::{
    context_graph::check_document_contexts,
    context_spec::ContextSpec,
    engine::{
        compaction_context::{CompactionContext, CompactionContextError, ContextEmbedding},
        expanded_text::ExpandedText,
        member_order::sort_members,
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
        processor_text::ProcessorTextError,
    },
    error::JsonLdError,
};
use andromeda_json_literal::piece::JsonPiece;
use iri_rs::IriBuf;
use jsonld::{
    CompactError,
    ExpandError,
    ExpandedDocument,
    ExpandedJsonTextError,
    Options,
    expansion::{Context as ActiveContext, SliceExpansionError, expand_slice},
};
use jsonld_core::{Node, Object};
use jstrict::{Print, native_stack::on_sufficient_stack};
use rdfx::{BlankIdBuf, vocabulary::no_vocabulary_mut};
use serde_json::value::RawValue;
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

impl ContextCompactor<'_> {
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
    /// `@context` is not a JSON-LD context, a context-loading variant when a
    /// context did not resolve, and [`JsonLdError::ProcessorText`] for text
    /// that is not JSON.
    pub async fn compact(&self, document: ExpandedText<'_>, options: CompactOptions) -> Result<Box<RawValue>, JsonLdError> {
        let expanded = self.read(document).await?;
        let prepared = self.prepared().await?;
        let embedding = match options.strip_context {
            ContextStripping::Strip => ContextEmbedding::Omit,
            ContextStripping::Keep => ContextEmbedding::Embed,
        };

        let mut compacted = match prepared.compact(&expanded, self.engine.loader(), embedding).await {
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
        sort_members(&mut compacted);
        let compacted = compacted;
        Ok(RawValue::from_string(compacted.compact_print().to_string())?)
    }

    /// Read an expanded document into the processor's own form.
    ///
    /// A canonical document is read as it stands, which costs a fraction of
    /// expanding it again. The direct reader refuses a lone value where
    /// expanded form has an array, and it keeps a member key that is not an
    /// absolute IRI where expansion would drop it; a document showing either
    /// is not what the broker's expansion produces, so it is logged and
    /// expanded like an assembled one, which keeps its compaction exactly what
    /// it was.
    async fn read(&self, document: ExpandedText<'_>) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, JsonLdError> {
        match document {
            ExpandedText::Assembled(text) => self.expand(text).await,
            ExpandedText::Canonical(text) => match ExpandedDocument::try_from_json_slice_in(no_vocabulary_mut(), text) {
                Ok(read) if every_member_key_is_an_identifier(&read) => Ok(read),
                Ok(_) => {
                    tracing::warn!("A canonical document carries a member key that is not an absolute IRI; expanding it");
                    self.expand(text).await
                }
                Err(ExpandedJsonTextError::Invalid(error)) => {
                    tracing::warn!(error = %error, "A canonical document is not in expanded form; expanding it");
                    self.expand(text).await
                }
                Err(ExpandedJsonTextError::Syntax(error)) => Err(ProcessorTextError::from(error).into()),
            },
        }
    }

    /// Take a document through the expansion algorithm into the processor's
    /// own form, normalizing it: a lone value becomes an array, and a member
    /// whose key is not an absolute IRI is dropped.
    async fn expand(&self, text: &[u8]) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, JsonLdError> {
        let document: &RawValue = serde_json::from_slice(text)?;
        check_document_contexts(self.engine.loader(), JsonPiece::new(document)).await?;
        expand_slice(
            text,
            no_vocabulary_mut(),
            ActiveContext::new(None),
            None,
            self.engine.loader(),
            Options::<IriBuf>::default().expansion_options(),
            (),
        )
        .await
        .map_err(|error| match error {
            SliceExpansionError::Syntax(error) => JsonLdError::from(ProcessorTextError::from(error)),
            SliceExpansionError::Expansion(error) => {
                tracing::error!(error = %error, "Compaction failed");
                JsonLdError::from(CompactError::Expand(ExpandError::Expansion(error)))
            }
        })
    }

    /// The processed request context, for a compaction that selects terms
    /// itself rather than handing a document to the processor.
    ///
    /// It is the context [`ContextCompactor::compact`] compacts against:
    /// processed once per compactor, kept on the engine while every document it
    /// was processed from stays what the loader resolves, and replaced by the
    /// core context alone, with the same warning, when the request context
    /// redefines a protected core term.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::InlineContextInvalid`] when the resolved
    /// `@context` is not a JSON-LD context, a context-loading variant when a
    /// context did not resolve, or [`JsonLdError::Compaction`] when it did not
    /// process.
    pub async fn processed_context(&self) -> Result<Arc<CompactionContext>, JsonLdError> {
        self.prepared().await.map(Arc::clone)
    }

    /// The processed request context, processing it on first use.
    ///
    /// A failed attempt is not remembered, so the next document tries again,
    /// exactly as it would have had it been the first.
    async fn prepared(&self) -> Result<&Arc<CompactionContext>, JsonLdError> {
        self.prepared.get_or_try_init(|| self.prepare()).await
    }

    /// Process the request context, or the core context alone when the request
    /// context redefines a protected core term.
    async fn prepare(&self) -> Result<Arc<CompactionContext>, JsonLdError> {
        let loader = self.engine.loader();
        let preparation = || CompactionContext::prepare(self.context, loader);
        match self
            .engine
            .compaction_contexts()
            .get_or_prepare(self.context.context_key(), loader, preparation)
            .await
        {
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
        let core_only = ContextSpec::core_only();
        let loader = self.engine.loader();
        self.engine
            .compaction_contexts()
            .get_or_prepare(core_only.context_key(), loader, || CompactionContext::prepare(&core_only, loader))
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

/// Whether every member key of every node in the document names an IRI or a
/// blank node, as a key expansion keeps does.
///
/// The direct reader keeps a key that is neither as an invalid identifier,
/// where expansion drops the member, so a document carrying one would compact
/// differently read than expanded.
fn every_member_key_is_an_identifier(document: &ExpandedDocument<IriBuf, BlankIdBuf>) -> bool {
    document.iter().all(|object| object_keys_are_identifiers(object.inner()))
}

/// Whether every member key below an object names an IRI or a blank node.
///
/// Recurses once per nesting level of the document, each level on sufficient
/// native stack.
fn object_keys_are_identifiers(object: &Object<IriBuf, BlankIdBuf>) -> bool {
    on_sufficient_stack(|| match object {
        Object::Value(_) => true,
        Object::List(list) => list.iter().all(|item| object_keys_are_identifiers(item.inner())),
        Object::Node(node) => node_keys_are_identifiers(node),
    })
}

/// Whether every member key of a node, and of every node below it, names an
/// IRI or a blank node.
fn node_keys_are_identifiers(node: &Node<IriBuf, BlankIdBuf>) -> bool {
    on_sufficient_stack(|| node_members_are_identifiers(node))
}

/// [`node_keys_are_identifiers`] on the stack the caller left.
fn node_members_are_identifiers(node: &Node<IriBuf, BlankIdBuf>) -> bool {
    node.properties()
        .iter()
        .all(|(key, objects)| key.is_valid() && objects.iter().all(|object| object_keys_are_identifiers(object.inner())))
        && node.reverse_properties().is_none_or(|reverse| {
            reverse
                .iter()
                .all(|(key, nodes)| key.is_valid() && nodes.iter().all(|node| node_keys_are_identifiers(node.inner())))
        })
        && node
            .graph()
            .is_none_or(|graph| graph.iter().all(|object| object_keys_are_identifiers(object.inner())))
        && node
            .included()
            .is_none_or(|included| included.iter().all(|node| node_keys_are_identifiers(node.inner())))
}

/// The identifier of a document's first node, for a failure diagnostic.
fn document_id(document: &ExpandedDocument<IriBuf, BlankIdBuf>) -> &str {
    document.iter().find_map(|object| object.id()).map_or("<unknown>", |id| id.as_str())
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::ContextCache,
        context_spec::ContextSpec,
        engine::{
            expanded_text::ExpandedText,
            options::{CompactOptions, ContextStripping},
            processor::JsonLdEngine,
        },
        error::JsonLdError,
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
        loader::replaceable_row::ReplaceableRow,
        test_documents::{expanded_document, literal},
    };
    use andromeda_ngsi_ld_vocabulary::core_context::NGSI_LD_CORE_CONTEXT_V16_URL;
    use iri_rs::{IriBuf, IriRefBuf};
    use jsonld::{
        JsonLdProcessor,
        RemoteContextReference,
        RemoteDocument,
        syntax::{Context as JsonLdContext, TryFromJson},
    };
    use jstrict::{Print, parse::parse_str_value};
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
        ContextSpec::from_body(literal(&json!({
            "Vehicle": "http://example.org/vehicle/Vehicle",
            "brandName": "http://example.org/vehicle/brandName",
            "speed": "http://example.org/vehicle/speed",
            "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"}
        })))
    }

    /// Reference compaction: re-expand the document and compact it against a context processed for it alone, removing the `@context` afterwards when it is not kept; the prepared path must match it byte for byte.
    async fn per_document_compaction(engine: &JsonLdEngine, document: Value, spec: &ContextSpec, options: CompactOptions) -> Value {
        let context = JsonLdContext::try_from_json(&parse_str_value(spec.resolved_context().text()).expect("JSON")).expect("a JSON-LD context");
        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
        let remote: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, parse_str_value(&document.to_string()).expect("JSON"));
        let compacted = remote
            .compact_with(
                &mut vocabulary,
                RemoteContextReference::Loaded(RemoteDocument::new(None, None, context)),
                engine.loader(),
            )
            .await
            .expect("per-document compaction succeeds");
        let mut compacted: Value = serde_json::from_str(&compacted.compact_print().to_string()).expect("JSON");
        if options.strip_context == ContextStripping::Strip
            && let Some(object) = compacted.as_object_mut()
        {
            object.remove("@context");
        }
        compacted
    }

    fn iri_ref(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    /// The identifier the stored `@context` below is held under.
    const STORED: &str = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// Recognises the root-relative path of [`STORED`] as that identifier.
    #[derive(Debug)]
    struct StoredPath;

    impl HostedContextRecognizer for StoredPath {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            if reference.as_str() == format!("/ngsi-ld/v1/jsonldContexts/{STORED}") {
                HostedReference::Own(STORED.parse().expect("a stored identifier"))
            } else {
                HostedReference::Unrelated
            }
        }
    }

    /// A body `@context` naming one of the tenant's own stored contexts by the
    /// root-relative path the broker returned is compacted against, exactly
    /// as expansion reads it: a request body has no base URL to resolve it
    /// against, so the path stands for the stored context.
    #[tokio::test]
    async fn a_root_relative_reference_to_a_stored_context_compacts_as_expansion_reads_it() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "http://example.org/vehicle/speed" } })));
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .with_l2_cache(Arc::clone(&row) as Arc<dyn ContextCache>)
            .hosted_contexts(Arc::new(StoredPath))
            .build()
            .expect("engine builds");
        let spec = ContextSpec::from_body(literal(&json!(format!("/ngsi-ld/v1/jsonldContexts/{STORED}"))));
        let entity = json!({ "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": { "type": "Property", "value": 40 } });
        let expanded = expanded_document(&engine, &entity, &spec).await.expect("expansion reads the reference");

        let compacted: Value = serde_json::from_str(
            engine
                .compactor(&spec)
                .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
                .await
                .expect("compaction reads the same reference")
                .get(),
        )
        .expect("JSON");

        pretty_assertions::assert_eq!(compacted["speed"]["value"], json!(40));
    }

    #[tokio::test]
    async fn prepared_compaction_matches_per_document_compaction_byte_for_byte() {
        let engine = offline_engine();
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");
        let specs = [ContextSpec::none(), ContextSpec::core_only(), example_body_context()];

        for spec in &specs {
            for options in [STRIP, KEEP] {
                let prepared: Value = serde_json::from_str(
                    engine
                        .compactor(spec)
                        .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), options)
                        .await
                        .expect("prepared compaction succeeds")
                        .get(),
                )
                .expect("JSON");
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
        let first = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");
        let second = expanded_document(
            &engine,
            &json!({"id": "urn:ngsi-ld:Vehicle:B", "type": "Vehicle", "speed": {"type": "Property", "value": 12}}),
            &ContextSpec::none(),
        )
        .await
        .expect("expansion succeeds");

        for document in [first, second] {
            let prepared: Value = serde_json::from_str(
                compactor
                    .compact(ExpandedText::Assembled(document.to_string().as_bytes()), STRIP)
                    .await
                    .expect("prepared compaction succeeds")
                    .get(),
            )
            .expect("JSON");
            pretty_assertions::assert_eq!(prepared, per_document_compaction(&engine, document, &spec, STRIP).await);
        }
    }

    #[tokio::test]
    async fn a_kept_context_is_the_only_member_a_stripped_document_lacks() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");

        let kept = compactor
            .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), KEEP)
            .await
            .expect("compaction succeeds");
        let mut kept: Value = serde_json::from_str(kept.get()).expect("JSON");
        let stripped: Value = serde_json::from_str(
            compactor
                .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
                .await
                .expect("compaction succeeds")
                .get(),
        )
        .expect("JSON");
        let context = kept.as_object_mut().and_then(|object| object.remove("@context"));

        let resolved: Value = serde_json::from_str(spec.resolved_context().text()).expect("JSON");
        pretty_assertions::assert_eq!(context, Some(resolved));
        pretty_assertions::assert_eq!(kept, stripped);
    }

    #[tokio::test]
    async fn one_compactor_processes_its_context_once_for_every_document() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");

        compactor
            .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
            .await
            .expect("first compaction succeeds");
        let first = Arc::clone(compactor.prepared.get().expect("the first document prepared the context"));
        compactor
            .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
            .await
            .expect("second compaction succeeds");
        let second = compactor.prepared.get().expect("the context stays prepared");

        assert!(Arc::ptr_eq(&first, second), "the second document must reuse the context the first one prepared");
    }

    #[tokio::test]
    async fn an_embedded_context_is_processed_once_across_compactors() {
        let engine = offline_engine();
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");
        let none = ContextSpec::none();
        let versioned = ContextSpec::from_link(iri_ref(NGSI_LD_CORE_CONTEXT_V16_URL.as_str()));
        let first = engine.compactor(&none);
        let second = engine.compactor(&versioned);

        first
            .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
            .await
            .expect("first compaction succeeds");
        second
            .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
            .await
            .expect("second compaction succeeds");

        assert!(Arc::ptr_eq(first.prepared.get().expect("prepared"), second.prepared.get().expect("prepared")));
        pretty_assertions::assert_eq!(engine.compaction_contexts().len().await, 1);
    }

    #[tokio::test]
    async fn a_body_context_is_processed_once_across_compactors() {
        let engine = offline_engine();
        let spec = example_body_context();
        let first = engine.compactor(&spec).processed_context().await.expect("the context processes");
        let second = engine.compactor(&spec).processed_context().await.expect("the context processes");

        assert!(Arc::ptr_eq(&first, &second), "the second compactor must reuse the processed context");
        pretty_assertions::assert_eq!(engine.compaction_contexts().len().await, 1);
    }

    /// A kept context is only as fresh as the documents it was processed from:
    /// once the loader reads a document again (its entry lapsed, clause
    /// 6.3.16), the next operation processes the document the loader holds now.
    #[tokio::test]
    async fn a_context_whose_document_is_read_again_is_processed_again() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "alpha": "http://example.org/a" } })));
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .with_l2_cache(Arc::clone(&row) as Arc<dyn ContextCache>)
            .build()
            .expect("engine builds");
        let url = iri_ref("https://example.org/stored.jsonld");
        let spec = ContextSpec::from_link(url.clone()); // clone: the invalidation below names the same URL
        let document = json!({"id": "urn:ngsi-ld:Thing:1", "type": "Thing", "http://example.org/a": {"type": "Property", "value": 1}});
        let expanded = expanded_document(&engine, &document, &ContextSpec::none()).await.expect("expansion succeeds");

        let first = engine.compactor(&spec).processed_context().await.expect("the context processes");
        let kept = engine.compactor(&spec).processed_context().await.expect("the context processes");
        assert!(Arc::ptr_eq(&first, &kept), "an unchanged document keeps its processed context");

        row.replace(&json!({ "@context": { "beta": "http://example.org/a" } }));
        engine.invalidate_context(&url).await;
        let compactor = engine.compactor(&spec);
        let reprocessed = compactor.processed_context().await.expect("the context processes");
        assert!(!Arc::ptr_eq(&first, &reprocessed), "the document read again is processed again");

        let compacted: Value = serde_json::from_str(
            compactor
                .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
                .await
                .expect("compaction succeeds")
                .get(),
        )
        .expect("JSON");
        assert!(compacted.get("beta").is_some(), "compacted under the document read again: {compacted}");
    }

    /// A caller selecting terms itself reads the very context the compactor
    /// compacts documents against, processed once for both.
    #[tokio::test]
    async fn the_processed_context_is_the_one_every_document_is_compacted_against() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");

        let processed = compactor.processed_context().await.expect("the context processes");
        compactor
            .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
            .await
            .expect("compaction succeeds");

        assert!(Arc::ptr_eq(&processed, compactor.prepared.get().expect("the context stays prepared")));
        assert!(processed.active_context().get("brandName").is_some(), "the request's own terms are defined");
    }

    /// A request context redefining a protected core term is handed out as
    /// the core context alone, exactly as documents are compacted against it.
    #[tokio::test]
    async fn the_processed_context_of_a_protected_term_redefinition_is_the_core_context() {
        let engine = offline_engine();
        let spec = ContextSpec::from_body(literal(&json!([
            {"@protected": true, "speed": "http://example.org/protected/speed"},
            {"speed": "http://example.org/redefined/speed"}
        ])));
        let core = ContextSpec::core_only();

        let processed = engine.compactor(&spec).processed_context().await.expect("the fallback processes");
        let core_processed = engine.compactor(&core).processed_context().await.expect("the core context processes");

        assert!(Arc::ptr_eq(&processed, &core_processed));
    }

    #[tokio::test]
    async fn a_compactor_that_compacts_nothing_never_resolves_its_context() {
        let engine = offline_engine();
        let spec = ContextSpec::from_link(iri_ref("https://example.com/unreachable.jsonld"));
        let compactor = engine.compactor(&spec);

        assert!(compactor.prepared.get().is_none());
    }

    #[tokio::test]
    async fn an_unresolvable_context_fails_every_document_and_is_never_remembered() {
        let engine = offline_engine();
        let spec = ContextSpec::from_link(iri_ref("https://example.com/unreachable.jsonld"));
        let compactor = engine.compactor(&spec);
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");

        let first = compactor.compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP).await;
        let second = compactor.compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP).await;

        assert!(first.as_ref().is_err_and(JsonLdError::is_context_not_available), "got {first:?}");
        assert!(second.as_ref().is_err_and(JsonLdError::is_context_not_available), "got {second:?}");
        assert!(compactor.prepared.get().is_none());
    }

    /// A context that redefines a term an earlier context of the request
    /// protected fails while it is processed (JSON-LD 1.1 section 4.1.11), so
    /// every document of the operation is compacted against the core context
    /// alone.
    #[tokio::test]
    async fn a_context_redefining_a_protected_term_compacts_every_document_against_the_core_context() {
        let engine = offline_engine();
        let spec = ContextSpec::from_body(literal(&json!([
            {"@protected": true, "speed": "http://example.org/protected/speed"},
            {"speed": "http://example.org/redefined/speed"}
        ])));
        let expanded = expanded_document(&engine, &normalized_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");

        let compacted: Value = serde_json::from_str(
            engine
                .compactor(&spec)
                .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), STRIP)
                .await
                .expect("the fallback compacts")
                .get(),
        )
        .expect("JSON");
        let core_only = per_document_compaction(&engine, expanded, &ContextSpec::core_only(), STRIP).await;

        pretty_assertions::assert_eq!(compacted, core_only);
    }

    /// A type-scoped context is processed only for a node of that type, so a
    /// protected-term redefinition inside one falls back for that document
    /// alone; a document of another type still gets the request's own terms.
    #[tokio::test]
    async fn a_type_scoped_protected_term_redefinition_falls_back_for_that_document_alone() {
        let engine = offline_engine();
        let spec = ContextSpec::from_body(literal(&json!({
            "Vehicle": {"@id": "http://example.org/vehicle/Vehicle", "@context": {"type": "http://example.org/redefined/type"}},
            "speed": "http://example.org/vehicle/speed"
        })));
        let compactor = engine.compactor(&spec);
        let vehicle = expanded_document(&engine, &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "http://example.org/vehicle/Vehicle", "http://example.org/vehicle/speed": {"type": "Property", "value": 1}}),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");
        let bicycle = expanded_document(&engine, &json!({"id": "urn:ngsi-ld:Bicycle:1", "type": "http://example.org/vehicle/Bicycle", "http://example.org/vehicle/speed": {"type": "Property", "value": 2}}),
                &ContextSpec::none(),
            )
            .await
            .expect("expansion succeeds");

        let vehicle_compacted: Value = serde_json::from_str(
            compactor
                .compact(ExpandedText::Assembled(vehicle.to_string().as_bytes()), STRIP)
                .await
                .expect("the fallback compacts")
                .get(),
        )
        .expect("JSON");
        let bicycle_compacted: Value = serde_json::from_str(
            compactor
                .compact(ExpandedText::Assembled(bicycle.to_string().as_bytes()), STRIP)
                .await
                .expect("compaction succeeds")
                .get(),
        )
        .expect("JSON");

        pretty_assertions::assert_eq!(
            vehicle_compacted,
            per_document_compaction(&engine, vehicle, &ContextSpec::core_only(), STRIP).await
        );
        assert!(
            bicycle_compacted.get("speed").is_some(),
            "the request context still applies: {bicycle_compacted}"
        );
    }

    /// A number no binary float spells exactly, which must survive both
    /// readings digit for digit.
    fn exact_number() -> Value {
        serde_json::from_str("0.100000000000000005").expect("an exact JSON number")
    }

    /// An entity carrying every attribute family, multi-instance `datasetId`
    /// attributes, nested sub-attributes, `unitCode`, `observedAt`, the
    /// attribute-instance system times a `sysAttrs` representation keeps, an
    /// `expiresAt` and a `scope`.
    fn every_family_entity() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:every-family",
            "type": ["Vehicle", "http://example.org/vehicle/Car"],
            "scope": ["/Madrid/Gardens", "/Company/Fleet"],
            "brandName": {
                "type": "Property", "value": "Mercedes", "observedAt": "2020-08-01T12:03:00Z",
                "createdAt": "2020-08-01T12:03:00.123456Z", "modifiedAt": "2020-08-02T12:03:00.5Z",
                "rating": {"type": "Property", "value": 4.5, "unitCode": "C62", "reviewedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1"}}
            },
            "speed": [
                {"type": "Property", "value": 55, "datasetId": "urn:ngsi-ld:Dataset:gps", "unitCode": "KMH"},
                {"type": "Property", "value": exact_number(), "datasetId": "urn:ngsi-ld:Dataset:obd", "createdAt": "2021-01-01T00:00:00Z"}
            ],
            "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:Downtown1", "objectType": "OffStreetParking", "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:Bob"}},
            "passengers": {"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Person:1"}, {"object": "urn:ngsi-ld:Person:2"}]},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "route": {"type": "GeoProperty", "value": {"type": "LineString", "coordinates": [[-8.5, 41.2], [-8.6, 41.3]]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt", "@none": "Main"}},
            "config": {"type": "JsonProperty", "json": {"nested": {"n": 0.5, "list": [1, "two", null, true]}}},
            "readings": {"type": "ListProperty", "valueList": [1, 2.5, "three"]},
            "category": {"type": "VocabProperty", "vocab": "commercial"},
            "tags": {"type": "Property", "value": ["a", "b"]},
            "nothing": {"type": "Property", "value": {"nested": null, "@language": "not-a-keyword-here"}},
            "expiresAt": "2099-01-01T00:00:00Z"
        })
    }

    /// The compact document with every `createdAt` and `modifiedAt` removed,
    /// as a representation that did not ask for `sysAttrs` carries it.
    fn without_system_times(document: Value) -> Value {
        match document {
            Value::Object(object) => Value::Object(
                object
                    .into_iter()
                    .filter(|(key, _)| key != "createdAt" && key != "modifiedAt")
                    .map(|(key, value)| (key, without_system_times(value)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.into_iter().map(without_system_times).collect()),
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
        }
    }

    /// Compact the same expanded document read directly as canonical and taken
    /// through expansion as assembled, rendered to JSON text.
    async fn both_readings(engine: &JsonLdEngine, spec: &ContextSpec, expanded: &Value, options: CompactOptions) -> (String, String) {
        let compactor = engine.compactor(spec);
        let canonical: Value = serde_json::from_str(
            compactor
                .compact(ExpandedText::Canonical(expanded.to_string().as_bytes()), options)
                .await
                .expect("canonical compaction succeeds")
                .get(),
        )
        .expect("JSON");
        let assembled: Value = serde_json::from_str(
            compactor
                .compact(ExpandedText::Assembled(expanded.to_string().as_bytes()), options)
                .await
                .expect("assembled compaction succeeds")
                .get(),
        )
        .expect("JSON");
        (
            serde_json::to_string(&canonical).expect("canonical JSON"),
            serde_json::to_string(&assembled).expect("assembled JSON"),
        )
    }

    /// A canonical document read directly compacts byte for byte as it does when
    /// taken through expansion, for every attribute family, with and without
    /// the attribute-instance system times, under every kind of context and
    /// either context embedding.
    #[tokio::test]
    async fn a_canonical_document_read_directly_compacts_exactly_as_expanded() {
        let engine = offline_engine();
        let with_times = expanded_document(&engine, &every_family_entity(), &ContextSpec::none())
            .await
            .expect("expansion succeeds");
        let without_times = expanded_document(&engine, &without_system_times(every_family_entity()), &ContextSpec::none())
            .await
            .expect("expansion succeeds");
        let type_scoped = ContextSpec::from_body(literal(&json!({
            "Vehicle": {"@id": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle", "@context": {"speed": "http://example.org/scoped/speed"}},
            "speed": "https://uri.etsi.org/ngsi-ld/default-context/speed"
        })));
        let protected_term_redefinition = ContextSpec::from_body(literal(&json!([
            {"@protected": true, "speed": "http://example.org/protected/speed"},
            {"speed": "http://example.org/redefined/speed"}
        ])));
        let specs = [
            ContextSpec::none(),
            ContextSpec::core_only(),
            example_body_context(),
            type_scoped,
            protected_term_redefinition,
        ];
        for expanded in [&with_times, &without_times] {
            for spec in &specs {
                for options in [STRIP, KEEP] {
                    let (canonical, assembled) = both_readings(&engine, spec, expanded, options).await;
                    pretty_assertions::assert_eq!(canonical, assembled, "spec {spec:?} with {options:?}");
                }
            }
        }
    }

    /// A canonical document carrying a lone value where expanded form has an
    /// array is refused by the direct reader and expanded instead, so it still
    /// compacts exactly as an assembled one.
    #[tokio::test]
    async fn a_canonical_document_with_a_lone_value_compacts_as_expanded() {
        let engine = offline_engine();
        let lone = json!([{
            "@id": "urn:ngsi-ld:Vehicle:lone",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/default-context/speed": {
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": {"@value": 12}
            }
        }]);
        for spec in [ContextSpec::none(), example_body_context()] {
            let (canonical, assembled) = both_readings(&engine, &spec, &lone, STRIP).await;
            pretty_assertions::assert_eq!(canonical, assembled);
        }
    }

    /// A canonical document carrying a member key that is not an absolute IRI is
    /// read by the direct reader with the member kept, where expansion drops
    /// it, so it is expanded instead and compacts exactly as an assembled one,
    /// wherever in the document the key sits.
    #[tokio::test]
    async fn a_canonical_document_with_a_relative_key_compacts_as_expanded() {
        let engine = offline_engine();
        let top_level = json!([{
            "@id": "urn:ngsi-ld:Vehicle:relative",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            "speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 12}]}]
        }]);
        let nested = json!([{
            "@id": "urn:ngsi-ld:Vehicle:relative-nested",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/default-context/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 12}],
                "accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
            }]
        }]);
        let (canonical, assembled) = both_readings(&engine, &ContextSpec::core_only(), &top_level, STRIP).await;
        pretty_assertions::assert_eq!(canonical, assembled);
        assert!(!canonical.contains("speed"), "the relative top-level key is dropped: {canonical}");
        let (canonical, assembled) = both_readings(&engine, &ContextSpec::core_only(), &nested, STRIP).await;
        pretty_assertions::assert_eq!(canonical, assembled);
        assert!(!canonical.contains("accuracy"), "the relative nested key is dropped: {canonical}");
    }
}
