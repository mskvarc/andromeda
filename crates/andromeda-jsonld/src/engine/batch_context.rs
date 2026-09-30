//! One `@context`, resolved once, reused across a batch.
//!
//! Every entity of a batch normally makes the engine rebuild the same
//! resolved `@context` and hand the same contexts to the processor again.
//! [`BatchContextHandle`] does that work once. It carries the resolved value
//! and the [`BatchTermMap`] the fastpath reads.
//!
//! Clause 5.5.7 lets an individual entity carry its own `@context`, and a
//! batch gives every distinct context its own handle: the entities naming one
//! context are prepared together under that context's handle, so no entity is
//! ever expanded against another's table.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    engine::{
        fast_entity::expand_document,
        processor::JsonLdEngine,
        term_map::{BatchTermMap, build_batch_term_map},
    },
    error::JsonLdError,
};
use serde_json::Value;
use std::sync::Arc;

/// A batch's `@context`, resolved.
#[derive(Clone, Debug)]
pub struct BatchContextHandle {
    /// The spec the handle was built from, for the per-entity fallback.
    spec: ContextSpec,
    /// The term table the fastpath resolves against.
    term_map: Arc<BatchTermMap>,
}

impl BatchContextHandle {
    /// Whether the fastpath may be attempted for entities of this batch.
    ///
    /// When `false`, every entity takes the processor's path regardless of
    /// its own shape.
    #[must_use]
    pub fn fast_eligible(&self) -> bool {
        self.term_map.is_eligible()
    }

    /// The [`ContextSpec`] this handle resolved.
    #[must_use]
    pub const fn spec(&self) -> &ContextSpec {
        &self.spec
    }

    /// The term table, for tests that assert what a context resolved to.
    #[cfg(test)]
    pub(crate) fn term_map(&self) -> &BatchTermMap {
        &self.term_map
    }
}

impl JsonLdEngine {
    /// Resolve one `@context` for a whole batch.
    ///
    /// Asynchronous because a request context named by URL is resolved
    /// through the loader, which may reach the registry, the durable cache or
    /// the network. Parsed context tables are kept on the engine, so a second
    /// batch naming the same URL pays a map read.
    pub async fn batch_context_handle(&self, context: &ContextSpec) -> BatchContextHandle {
        let term_map = build_batch_term_map(context, self.loader(), self.user_term_tables(), self.core_vocabulary()).await;

        BatchContextHandle {
            spec: context.clone(), // clone: the handle outlives the caller's borrow
            term_map: Arc::new(term_map),
        }
    }

    /// Expand many already-prepared documents in one processor call.
    ///
    /// Every input must already carry its effective `@context`. Wrapping them
    /// in one array means one vocabulary and one loader handshake for the
    /// whole batch; the result is split back apart in input order.
    ///
    /// A whole-batch failure — one malformed entity poisons the call — is
    /// retried per entity so the failure lands on the input that caused it.
    /// That costs a second pass only on malformed input.
    pub async fn expand_prepared_array(&self, documents: Vec<Value>, core_policy: CoreContextPolicy) -> Vec<Result<Value, JsonLdError>> {
        if documents.is_empty() {
            return Vec::new();
        }
        let count = documents.len();

        let bulk = Value::Array(documents.clone()); // clone: the per-entity retry needs the originals
        match self.expand_prepared(bulk, core_policy).await {
            // Each node object is re-wrapped as a one-element array so a
            // caller cannot tell a bulk expansion from a single one.
            Ok(Value::Array(items)) if items.len() == count => items.into_iter().map(|item| Ok(Value::Array(vec![item]))).collect(),
            Ok(_reshaped) => {
                tracing::debug!("Bulk expansion shape mismatch for {} inputs; retrying per-entity", count);
                self.expand_each(documents, core_policy).await
            }
            Err(bulk_error) => {
                tracing::warn!(error = %bulk_error, "Bulk JSON-LD expansion failed; retrying per-entity to attribute error");
                self.expand_each(documents, core_policy).await
            }
        }
    }

    /// Expand each document on its own, so a failure names its input.
    async fn expand_each(&self, documents: Vec<Value>, core_policy: CoreContextPolicy) -> Vec<Result<Value, JsonLdError>> {
        let mut results = Vec::with_capacity(documents.len());
        for document in documents {
            results.push(self.expand_prepared(document, core_policy).await);
        }
        results
    }

    /// The fastpath alone, performing no I/O.
    ///
    /// Answers `None` when the handle or the document is outside the
    /// supported subset, so the caller can route the entity to the
    /// asynchronous path. Safe to call from a blocking pool.
    #[must_use]
    pub fn try_fast_expand(&self, document: &Value, handle: &BatchContextHandle) -> Option<Value> {
        if !handle.term_map.is_eligible() {
            return None;
        }
        expand_document(document, &handle.term_map)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::{ContextSpec, CoreContextPolicy},
        engine::{processor::JsonLdEngine, term_map::BatchTermMap},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// One document of a bulk expansion, distinguishable by index.
    fn bulk_document(index: usize) -> Value {
        json!({
            "@context": NGSI_LD_CORE_CONTEXT_URL,
            "id": format!("urn:ngsi-ld:Device:bulk-{index}"),
            "type": "Device"
        })
    }

    #[tokio::test]
    async fn a_spec_naming_only_core_contexts_admits_the_fastpath() {
        let engine = offline_engine();

        for spec in [
            ContextSpec::none(),
            ContextSpec::core_only(),
            ContextSpec::from_link(iri_ref(NGSI_LD_CORE_CONTEXT_URL.as_str())),
        ] {
            assert!(
                engine.batch_context_handle(&spec).await.fast_eligible(),
                "a core-only spec must admit the fastpath: {spec:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_body_context_that_only_declares_a_vocabulary_still_admits_the_fastpath() {
        let spec = ContextSpec::from_body(json!({ "@vocab": "https://example.com/vocab#" }));

        let handle = offline_engine().batch_context_handle(&spec).await;

        assert!(handle.fast_eligible(), "every term still resolves to one unambiguous IRI");
    }

    #[tokio::test]
    async fn a_context_that_redefines_a_core_term_disables_the_fastpath() {
        let spec = ContextSpec::from_body(json!({ "observedAt": "https://example.com/own/observedAt" }));

        let handle = offline_engine().batch_context_handle(&spec).await;

        assert!(!handle.fast_eligible(), "a redefined core term makes the two paths disagree");
    }

    #[tokio::test]
    async fn a_context_url_that_does_not_resolve_disables_the_fastpath() {
        let spec = ContextSpec::from_link(iri_ref("https://example.com/custom.jsonld"));

        let handle = offline_engine().batch_context_handle(&spec).await;

        assert!(!handle.fast_eligible(), "an unresolved context defines terms the term map does not have");
    }

    #[tokio::test]
    async fn the_term_map_is_shareable_across_a_blocking_pool() {
        // The fastpath runs on a blocking pool, which requires the term map to
        // cross a thread boundary; a compile-time assertion is the only kind
        // that catches a field being added that does not.
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BatchTermMap>();
    }

    #[tokio::test]
    async fn a_bulk_expansion_answers_exactly_what_expanding_each_document_answers() {
        let engine = offline_engine();
        let documents: Vec<Value> = (0..10).map(bulk_document).collect();

        let bulk = engine
            .expand_prepared_array(documents.clone(), CoreContextPolicy::Append) // clone: each document is expanded again below
            .await;

        pretty_assertions::assert_eq!(bulk.len(), documents.len());
        for (index, (bulk_result, document)) in bulk.iter().zip(documents.iter()).enumerate() {
            let bulk_value = bulk_result.as_ref().expect("every document expands");
            let solo = engine
                .expand(document.clone(), &ContextSpec::none()) // clone: expansion consumes the document
                .await
                .expect("every document expands");
            pretty_assertions::assert_eq!(bulk_value, &solo, "bulk[{index}] must match the individual expansion");
        }
    }

    #[tokio::test]
    async fn a_bulk_expansion_attributes_a_failure_to_the_document_that_caused_it() {
        let engine = offline_engine();
        let mut documents: Vec<Value> = (0..10).map(bulk_document).collect();
        documents[5] = json!({
            "@context": "https://example.com/missing-context.jsonld",
            "id": "urn:ngsi-ld:Device:bulk-5",
            "type": "Device"
        });

        let bulk = engine.expand_prepared_array(documents, CoreContextPolicy::Append).await;

        pretty_assertions::assert_eq!(bulk.len(), 10);
        for (index, slot) in bulk.iter().enumerate() {
            if index == 5 {
                assert!(slot.is_err(), "the unresolvable context must fail at its own index");
            } else {
                assert!(slot.is_ok(), "document {index} must succeed, got {slot:?}");
            }
        }
    }

    #[tokio::test]
    async fn a_bulk_expansion_of_nothing_answers_nothing() {
        let bulk = offline_engine().expand_prepared_array(Vec::new(), CoreContextPolicy::Append).await;
        assert!(bulk.is_empty());
    }

    #[tokio::test]
    async fn a_bulk_expansion_of_one_document_answers_the_same_as_expanding_it_alone() {
        let engine = offline_engine();
        let document = bulk_document(0);

        let bulk = engine
            .expand_prepared_array(vec![document.clone()], CoreContextPolicy::Append) // clone: expanded alone below
            .await;
        let solo = engine.expand(document, &ContextSpec::none()).await.expect("expansion succeeds");

        pretty_assertions::assert_eq!(bulk.len(), 1);
        pretty_assertions::assert_eq!(bulk[0].as_ref().expect("the single document expands"), &solo);
    }
}
