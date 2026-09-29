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
        expanded_input::ExpandedInput,
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
    error::JsonLdError,
};
use iri_rs::IriBuf;
use jsonld::{CompactError, ExpandedDocument, JsonLdProcessor, Options, RemoteDocument};
use jsonld_core::{Node, Object, TryFromJson};
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
    pub async fn compact(&self, document: ExpandedInput, options: CompactOptions) -> Result<Value, JsonLdError> {
        let expanded = self.read(document).await?;
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
    /// A canonical document is read as it stands, which costs a fraction of
    /// expanding it again. The direct reader refuses a lone value where
    /// expanded form has an array, and it keeps a member key that is not an
    /// absolute IRI where expansion would drop it; a document showing either
    /// is not what the broker's expansion produces, so it is logged and
    /// expanded like an assembled one, which keeps its compaction exactly what
    /// it was.
    async fn read(&self, document: ExpandedInput) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, JsonLdError> {
        match document {
            ExpandedInput::Assembled(document) => self.expand(jsonld_syntax::Value::from_serde_json(document)).await,
            ExpandedInput::Canonical(document) => {
                let json = jsonld_syntax::Value::from_serde_json(document);
                // clone: the direct reader consumes the document, and the one
                // it cannot take as it stands is expanded instead.
                match ExpandedDocument::try_from_json_in(no_vocabulary_mut(), json.clone()) {
                    Ok(read) if every_member_key_is_an_identifier(&read) => Ok(read),
                    Ok(_) => {
                        tracing::warn!("A canonical document carries a member key that is not an absolute IRI; expanding it");
                        self.expand(json).await
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "A canonical document is not in expanded form; expanding it");
                        self.expand(json).await
                    }
                }
            }
        }
    }

    /// Take a document through the expansion algorithm into the processor's
    /// own form, normalizing it: a lone value becomes an array, and a member
    /// whose key is not an absolute IRI is dropped.
    async fn expand(&self, document: jsonld_syntax::Value) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, JsonLdError> {
        let remote: RemoteDocument<IriBuf> = RemoteDocument::new(None, None, document);
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
fn object_keys_are_identifiers(object: &Object<IriBuf, BlankIdBuf>) -> bool {
    match object {
        Object::Value(_) => true,
        Object::List(list) => list.iter().all(|item| object_keys_are_identifiers(item.inner())),
        Object::Node(node) => node_keys_are_identifiers(node),
    }
}

/// Whether every member key of a node, and of every node below it, names an
/// IRI or a blank node.
fn node_keys_are_identifiers(node: &Node<IriBuf, BlankIdBuf>) -> bool {
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
        context_spec::{ContextSpec, CoreContextPolicy},
        core_context::NGSI_LD_CORE_CONTEXT_V16_URL,
        engine::{
            expanded_input::ExpandedInput,
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
                    .compact(ExpandedInput::Assembled(expanded.clone()), options)
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
            let prepared = compactor
                .compact(ExpandedInput::Assembled(document.clone()), STRIP)
                .await
                .expect("prepared compaction succeeds");
            pretty_assertions::assert_eq!(prepared, per_document_compaction(&engine, document, &spec, STRIP).await);
        }
    }

    #[tokio::test]
    async fn a_kept_context_is_the_only_member_a_stripped_document_lacks() {
        let engine = offline_engine();
        let spec = example_body_context();
        let compactor = engine.compactor(&spec);
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        let mut kept = compactor
            .compact(ExpandedInput::Assembled(expanded.clone()), KEEP)
            .await
            .expect("compaction succeeds");
        let stripped = compactor.compact(ExpandedInput::Assembled(expanded), STRIP).await.expect("compaction succeeds");
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

        compactor
            .compact(ExpandedInput::Assembled(expanded.clone()), STRIP)
            .await
            .expect("first compaction succeeds");
        let first = Arc::clone(compactor.prepared.get().expect("the first document prepared the context"));
        compactor
            .compact(ExpandedInput::Assembled(expanded), STRIP)
            .await
            .expect("second compaction succeeds");
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

        first
            .compact(ExpandedInput::Assembled(expanded.clone()), STRIP)
            .await
            .expect("first compaction succeeds");
        second
            .compact(ExpandedInput::Assembled(expanded), STRIP)
            .await
            .expect("second compaction succeeds");

        assert!(Arc::ptr_eq(first.prepared.get().expect("prepared"), second.prepared.get().expect("prepared")));
        pretty_assertions::assert_eq!(engine.compaction_contexts().len().await, 1);
    }

    #[tokio::test]
    async fn a_body_context_is_processed_per_compactor_and_never_kept_on_the_engine() {
        let engine = offline_engine();
        let spec = example_body_context();
        let expanded = engine.expand(normalized_entity(), &ContextSpec::none()).await.expect("expansion succeeds");

        engine
            .compactor(&spec)
            .compact(ExpandedInput::Assembled(expanded), STRIP)
            .await
            .expect("compaction succeeds");

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

        let first = compactor.compact(ExpandedInput::Assembled(expanded.clone()), STRIP).await;
        let second = compactor.compact(ExpandedInput::Assembled(expanded), STRIP).await;

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

        let compacted = engine
            .compactor(&spec)
            .compact(ExpandedInput::Assembled(expanded.clone()), STRIP)
            .await
            .expect("the fallback compacts");
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

        let vehicle_compacted = compactor
            .compact(ExpandedInput::Assembled(vehicle.clone()), STRIP)
            .await
            .expect("the fallback compacts");
        let bicycle_compacted = compactor.compact(ExpandedInput::Assembled(bicycle), STRIP).await.expect("compaction succeeds");

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
        let canonical = compactor
            .compact(ExpandedInput::Canonical(expanded.clone()), options)
            .await
            .expect("canonical compaction succeeds");
        let assembled = compactor
            .compact(ExpandedInput::Assembled(expanded.clone()), options)
            .await
            .expect("assembled compaction succeeds");
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
        let with_times = engine.expand(every_family_entity(), &ContextSpec::none()).await.expect("expansion succeeds");
        let without_times = engine
            .expand(without_system_times(every_family_entity()), &ContextSpec::none())
            .await
            .expect("expansion succeeds");
        let type_scoped = ContextSpec::none().with_body_context(Some(json!({
            "Vehicle": {"@id": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle", "@context": {"speed": "http://example.org/scoped/speed"}},
            "speed": "https://uri.etsi.org/ngsi-ld/default-context/speed"
        })));
        let protected_term_redefinition = ContextSpec::none()
            .with_body_context(Some(json!([NGSI_LD_CORE_CONTEXT_URL.as_str(), {"type": "http://example.org/redefined/type"}])))
            .with_core_policy(CoreContextPolicy::Omit);
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
