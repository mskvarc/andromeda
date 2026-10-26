//! A request `@context`, processed once into the active context expansion
//! reads.
//!
//! The JSON-LD processor turns the `@context` a [`ContextSpec`] resolves to
//! (clause 5.5.7: the request's own context first, the core context appended
//! last) into an active context before it reads a single member of a document.
//! Nothing of that depends on the document, so [`ExpansionContext`] does it
//! once and keeps the result: a typed expansion of the request decides every
//! term against [`ExpansionContext::active`], a single term is expanded
//! against it, and the processor, when a document
//! needs it, starts from the same active context instead of processing the
//! `@context` again ([`ExpansionContext::expand_text`]).
//!
//! JSON-LD 1.1 applies a term's property-scoped context to the object written
//! under that term. The one the core context declares (`ngsildproof`, clause
//! 4.5.2.2) and any a context source's context declares are processed here, on
//! top of the root active context, so a synchronous walker can descend into
//! them without a loader.

#[cfg(test)]
use crate::engine::processor::count_document_expansion;
use crate::{
    context_graph::check_context_graph,
    context_spec::ContextSpec,
    core_term_aliases::core_term_aliases,
    document_context::{ReferenceReading, resolved_context},
    engine::{context_syntax::context_syntax, processing_warning::LoggedWarnings, processor::JsonLdEngine, processor_text::ProcessorTextError},
    error::JsonLdError,
    loader::{error::LoaderError, http::HttpContextLoader},
};
use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
use iri_rs::IriBuf;
use jsonld::{
    Context as ActiveContext,
    ExpandError,
    ExpandedDocument,
    context::BindingRef,
    context_processing::{Error as ProcessingError, Options as ProcessingOptions, Process},
    expansion::{Error as ExpansionAlgorithmError, Options as ExpansionOptions, SliceExpansionError, expand_slice},
};
use moka::future::Cache;
use rdfx::{BlankIdBuf, vocabulary::no_vocabulary_mut};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

/// How many distinct processed contexts are kept.
///
/// Only combinations of the released core contexts qualify, so the working
/// set is a handful of entries.
const CAPACITY: u64 = 16;

/// Processed contexts built only from documents embedded in the broker, keyed
/// by the resolved `@context` they were processed from.
///
/// Process-wide rather than per engine: an embedded core context is the same
/// document for every engine of the process, and no engine can make a core
/// context URL resolve to anything else. Kept for the life of the process: the
/// term memo of each kept context is capped, so its size does not grow with the
/// attribute names written.
static EMBEDDED_CONTEXTS: LazyLock<Cache<Arc<str>, Arc<ExpansionContext>>> = LazyLock::new(|| Cache::builder().max_capacity(CAPACITY).build());

/// The active context one request is expanded under.
#[derive(derive_more::Debug)]
pub struct ExpansionContext {
    /// The active context the request's `@context` processed into.
    #[debug(skip)]
    active: ActiveContext<IriBuf, BlankIdBuf>,
    /// The property-scoped context of every root term that declares one,
    /// processed on top of [`ExpansionContext::active`].
    #[debug(skip)]
    property_scoped: HashMap<Box<str>, ActiveContext<IriBuf, BlankIdBuf>>,
    /// The terms [`ExpansionContext::active`] defines for core terms, which a
    /// compact document written under it is read through.
    aliases: Arc<CoreTermAliases>,
}

impl ExpansionContext {
    /// The active context every term of the request is decided against.
    #[must_use]
    pub const fn active(&self) -> &ActiveContext<IriBuf, BlankIdBuf> {
        &self.active
    }

    /// The terms the context defines for core terms, which a compact
    /// document written under it is read through.
    #[must_use]
    pub const fn aliases(&self) -> &Arc<CoreTermAliases> {
        &self.aliases
    }

    /// The active context JSON-LD 1.1 expansion switches to for an object
    /// written under `term`, when `term` is defined at the root of
    /// [`ExpansionContext::active`] with a property-scoped context.
    ///
    /// Valid only for an object reached through root-level terms alone: a
    /// scoped context entered on the way changes what the next one is
    /// processed on top of.
    #[must_use]
    pub fn property_scoped(&self, term: &str) -> Option<&ActiveContext<IriBuf, BlankIdBuf>> {
        self.property_scoped.get(term)
    }

    /// Expand the JSON text of a compact document with the JSON-LD processor,
    /// starting from this active context.
    ///
    /// The document is expected to carry no `@context` of its own: the
    /// request's context is the one this was processed from. One it does carry
    /// is processed on top, as the processor always does. The text is parsed by
    /// the processor's own non-recursive reader and dropped before this
    /// returns.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::ProcessorText`] for text that is not JSON, and
    /// the expansion failure, typed as the processor types it, otherwise.
    pub async fn expand_text(&self, engine: &JsonLdEngine, text: &[u8]) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, JsonLdError> {
        #[cfg(test)]
        count_document_expansion();
        expand_slice(
            text,
            no_vocabulary_mut(),
            self.active.clone(), // clone: cheap, every definition and cache is shared through an `Arc`
            None,
            engine.loader(),
            ExpansionOptions::default(),
            (),
        )
        .await
        .map_err(|error| match error {
            SliceExpansionError::Syntax(error) => JsonLdError::from(ProcessorTextError::from(error)),
            SliceExpansionError::Expansion(error) => JsonLdError::from(ExpandError::Expansion(error)),
        })
    }

    /// Resolve and process the `@context` a spec selects, as a document
    /// written under it is processed.
    async fn prepare(spec: &ContextSpec, loader: &HttpContextLoader) -> Result<ExpansionContext, JsonLdError> {
        let resolved = resolved_context(spec, ReferenceReading::Hosted(loader.hosted_contexts()));
        check_context_graph(loader, &resolved).await?;
        let unprocessed = context_syntax(&resolved).map_err(|error| {
            // A context that is no JSON-LD context is reported as the
            // processor's expansion reports it.
            if let JsonLdError::InlineContextInvalid(invalid) = error {
                JsonLdError::from(ExpandError::Expansion(ExpansionAlgorithmError::ContextSyntax(invalid)))
            } else {
                error
            }
        })?;
        let active = unprocessed
            .process_full(
                no_vocabulary_mut(),
                &ActiveContext::new(None),
                loader,
                None,
                processing_options(),
                LoggedWarnings,
            )
            .await
            .map_err(processing_failure)?
            .into_processed();
        let property_scoped = property_scoped_contexts(&active, loader).await;
        let aliases = Arc::new(core_term_aliases(&active));
        Ok(ExpansionContext {
            active,
            property_scoped,
            aliases,
        })
    }
}

impl JsonLdEngine {
    /// The active context a document written under `spec` is expanded under.
    ///
    /// A context built only from documents the broker embeds is kept for the
    /// whole process; any other is kept on this engine while every document
    /// it was processed from stays what the loader resolves.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::ContextGraph`] for references that cycle or nest
    /// too deep, the context-loading variant of a context that does not
    /// resolve, and the processing failure of one that does not process.
    pub async fn expansion_context(&self, spec: &ContextSpec) -> Result<Arc<ExpansionContext>, JsonLdError> {
        let Some(key) = spec.embedded_context_key() else {
            let loader = self.loader();
            return self
                .expansion_contexts()
                .get_or_prepare(spec.context_key(), loader, || ExpansionContext::prepare(spec, loader))
                .await;
        };
        if let Some(cached) = EMBEDDED_CONTEXTS.get(&key).await {
            return Ok(cached);
        }
        let prepared = Arc::new(ExpansionContext::prepare(spec, self.loader()).await?);
        EMBEDDED_CONTEXTS.insert(key, Arc::clone(&prepared)).await; // clone: Arc ref-count bump
        Ok(prepared)
    }
}

/// The context processing options expansion processes a document's
/// `@context` with.
fn processing_options() -> ProcessingOptions {
    ProcessingOptions::from(ExpansionOptions::default())
}

/// A context-processing failure, typed as the processor's expansion reports
/// it.
fn processing_failure(error: ProcessingError<LoaderError>) -> JsonLdError {
    JsonLdError::from(ExpandError::Expansion(ExpansionAlgorithmError::ContextProcessing(error)))
}

/// Process the property-scoped context of every root term that declares one.
///
/// A scoped context that does not process is left out rather than failing the
/// request: expansion processes it only for an object actually written under
/// that term, and a walker meeting a term it holds no context for leaves the
/// document to the processor, which reports the failure where it occurs.
async fn property_scoped_contexts(
    active: &ActiveContext<IriBuf, BlankIdBuf>,
    loader: &HttpContextLoader,
) -> HashMap<Box<str>, ActiveContext<IriBuf, BlankIdBuf>> {
    let mut scoped = HashMap::new();
    for binding in active.definitions() {
        let BindingRef::Normal(term, definition) = binding else {
            continue;
        };
        let Some(local) = definition.context.as_deref() else {
            continue;
        };
        let base_url = definition.base_url().cloned(); // clone: processing takes the base URL by value
        if let Ok(processed) = local
            .process_full(
                no_vocabulary_mut(),
                active,
                loader,
                base_url,
                processing_options().with_override(),
                LoggedWarnings,
            )
            .await
        {
            scoped.insert(Box::from(term.as_str()), processed.into_processed());
        }
    }
    scoped
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_cache::ContextCache,
        context_spec::ContextSpec,
        engine::{preloaded_engine::engine_preloading, processor::JsonLdEngine},
        loader::replaceable_row::ReplaceableRow,
        operation_hit_scope::run_in_operation_scope,
        test_documents::literal,
    };
    use iri_rs::IriBuf;
    use jsonld::{
        Context,
        Environment,
        Id,
        NoLoader,
        Term,
        context::MAX_ENTRIES,
        context_processing::{
            Warning,
            algorithm::{Action, expand_iri_simple},
        },
        syntax::{ExpandableRef, IntoJsonWithContext, Nullable},
    };
    use jstrict::Print;
    use rdfx::{
        BlankIdBuf,
        vocabulary::{no_vocabulary, no_vocabulary_mut},
    };
    use serde_json::json;
    use std::{sync::Arc, thread};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The IRI `term` expands to as an expanded member key.
    fn key_iri(context: &Context<IriBuf, BlankIdBuf>, term: &str) -> String {
        let mut warnings = ();
        let mut environment = Environment {
            vocabulary: no_vocabulary_mut(),
            loader: &NoLoader,
            warnings: &mut warnings,
        };
        let expanded = expand_iri_simple::<Warning, _, _, _>(
            &mut environment,
            context,
            Nullable::Some(ExpandableRef::String(term)),
            false,
            Some(Action::Keep),
        )
        .expect("keep never rejects")
        .expect("keep never drops");
        match expanded.as_ref() {
            Term::Id(Id::Valid(id)) => id.to_string(),
            Term::Null | Term::Id(Id::Invalid(_)) | Term::Keyword(_) => panic!("{term} expands to no IRI"),
        }
    }

    #[tokio::test]
    async fn the_shared_core_contexts_term_memo_stays_capped_under_more_distinct_terms_than_the_cap() {
        let engine = offline_engine();
        let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");

        for i in 0..2 * MAX_ENTRIES {
            key_iri(context.active(), &format!("attribute{i}"));
        }

        let memoised = context.active().term_resolution_cache().len();
        assert!(memoised > 0 && memoised <= MAX_ENTRIES, "the term memo holds {memoised} entries");
        pretty_assertions::assert_eq!(key_iri(context.active(), "location"), "https://uri.etsi.org/ngsi-ld/location");
    }

    #[tokio::test]
    async fn the_core_context_decides_a_core_term_and_the_default_vocabulary_the_rest() {
        let engine = offline_engine();
        let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");

        pretty_assertions::assert_eq!(key_iri(context.active(), "location"), "https://uri.etsi.org/ngsi-ld/location");
        pretty_assertions::assert_eq!(key_iri(context.active(), "speed"), "https://uri.etsi.org/ngsi-ld/default-context/speed");
    }

    /// Clause 5.5.7: the core context is appended last, so a user context
    /// redefining a core term leaves the core definition in force.
    #[tokio::test]
    async fn a_user_context_cannot_redefine_a_core_term() {
        let engine = offline_engine();
        let spec = ContextSpec::from_body(literal(
            &json!({ "location": "https://example.org/location", "speed": "https://example.org/speed" }),
        ));
        let context = engine.expansion_context(&spec).await.expect("the context processes");

        pretty_assertions::assert_eq!(key_iri(context.active(), "location"), "https://uri.etsi.org/ngsi-ld/location");
        pretty_assertions::assert_eq!(key_iri(context.active(), "speed"), "https://example.org/speed");
    }

    #[tokio::test]
    async fn the_core_proof_term_carries_its_processed_scoped_context() {
        let engine = offline_engine();
        let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");

        let scoped = context.property_scoped("ngsildproof").expect("ngsildproof declares a scoped context");
        pretty_assertions::assert_eq!(key_iri(scoped, "entityIdSealed"), "https://uri.etsi.org/ngsi-ld/entityIdSealed");
        assert!(context.property_scoped("location").is_none());
    }

    #[tokio::test]
    async fn a_context_preloaded_by_url_is_processed_before_the_core_context() {
        let engine = engine_preloading(&[(
            "https://example.org/ctx.jsonld".to_owned(),
            json!({ "@context": { "speed": "https://example.org/speed" } }),
        )]);
        let spec = ContextSpec::from_link(IriBuf::new("https://example.org/ctx.jsonld".to_owned()).expect("an IRI"));
        let context = engine.expansion_context(&spec).await.expect("the context processes");

        pretty_assertions::assert_eq!(key_iri(context.active(), "speed"), "https://example.org/speed");
    }

    /// An engine reading every context from one stored row it can replace.
    fn engine_over(row: &Arc<ReplaceableRow>) -> JsonLdEngine {
        JsonLdEngine::builder()
            .allow_remote(false)
            .with_l2_cache(Arc::clone(row) as Arc<dyn ContextCache>)
            .build()
            .expect("engine builds")
    }

    /// A context named by URL is processed once while the loader holds the
    /// same document, and processed again once the loader has read the
    /// document again (clause 6.3.16 lets the stored copy lapse).
    #[tokio::test]
    async fn a_context_named_by_url_is_processed_again_only_once_its_document_is_read_again() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let engine = engine_over(&row);
        let url = IriBuf::new("https://example.org/stored.jsonld".to_owned()).expect("an IRI");
        let spec = ContextSpec::from_link(url.clone()); // clone: the invalidation below names the same URL

        let first = engine.expansion_context(&spec).await.expect("the context processes");
        let kept = engine.expansion_context(&spec).await.expect("the context processes");
        assert!(Arc::ptr_eq(&first, &kept), "an unchanged document keeps its processed context");

        row.replace(&json!({ "@context": { "speed": "https://example.org/velocity" } }));
        engine.invalidate_context(&url).await;
        let reprocessed = engine.expansion_context(&spec).await.expect("the context processes");

        pretty_assertions::assert_eq!(key_iri(reprocessed.active(), "speed"), "https://example.org/velocity");
    }

    /// Clause 5.13.3.5: an operation reusing a kept context finds the stored
    /// `@context` exactly as one processing it does, so each operation counts
    /// one hit.
    #[tokio::test]
    async fn every_operation_reusing_a_kept_context_counts_one_hit() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let engine = engine_over(&row);
        let url = IriBuf::new("https://example.org/stored.jsonld".to_owned()).expect("an IRI");
        let spec = ContextSpec::from_link(url.clone()); // clone: the counter is read under the same URL

        for _ in 0..3 {
            Box::pin(run_in_operation_scope(engine.expansion_context(&spec)))
                .await
                .expect("the context processes");
        }

        let counter = engine.hit_counter().expect("a durable cache attaches a hit counter");
        pretty_assertions::assert_eq!(counter.pending(&url), 3);
    }

    /// The processor reads the document text with its own reader, from the
    /// processed context, and keeps each number's exact text.
    #[tokio::test]
    async fn the_processor_expands_document_text_from_the_processed_context() {
        let engine = offline_engine();
        let text = r#"{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 0.100000000000000005}}"#;
        let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");

        let expanded = context.expand_text(&engine, text.as_bytes()).await.expect("the processor expands");
        let written = expanded.into_json_with(no_vocabulary()).compact_print().to_string();

        assert!(written.contains("0.100000000000000005"), "{written}");
        assert!(written.contains("https://uri.etsi.org/ngsi-ld/default-context/speed"), "{written}");
    }

    /// The expanded tree of a deeply nested document drops level by level: a
    /// recursive drop of its 2,000 nested node objects overflows the 256 KiB
    /// stack of the thread it is dropped on.
    #[tokio::test]
    async fn a_deep_processor_document_drops_on_a_small_stack() {
        const LEVELS: usize = 2_000;
        let engine = offline_engine();
        let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");
        let text = format!("{}{{}}{}", r#"{"https://e.org/p": "#.repeat(LEVELS), "}".repeat(LEVELS));

        let printed = context
            .expand_text(&engine, text.as_bytes())
            .await
            .expect("the deep document expands")
            .into_json_with(no_vocabulary())
            .compact_print()
            .to_string();
        pretty_assertions::assert_eq!(printed.matches("https://e.org/p").count(), LEVELS);
        let expanded = context.expand_text(&engine, text.as_bytes()).await.expect("the deep document expands");

        let dropped = thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || drop(expanded))
            .expect("the thread starts")
            .join();
        assert!(dropped.is_ok(), "the expanded tree dropped without overflowing");
    }
}
