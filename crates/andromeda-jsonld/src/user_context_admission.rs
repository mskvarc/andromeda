//! Admitting the user `@context` a client request submitted.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` shall
//! not "contain JSON-LD Scoped Contexts (see [2], section 4.1.8)", and that
//! "parts of user @context that are not following the two points above should
//! result in an error of type `BadRequestData`, because JSON-LD Scoped Contexts
//! and nested embedded @context could be used to modify terms defined in the
//! Core @context or to reshape NGSI-LD Elements during the expansion of terms".
//! Clauses 4.4 and 4.5.1 restate it. Clause 5.5.4 judges a payload
//! "syntactically correct, according to the @context", so the context is
//! resolved before anything is judged under it: one that cannot be retrieved is
//! clause 5.5.6's `LdContextNotAvailable`, decided first, and only a context
//! that loaded is inspected for what clause 5.5.7 refuses.
//!
//! The admission is enforced by type rather than by convention. A request
//! extractor yields a [`SubmittedContext`], which exposes nothing of the spec
//! it wraps; [`admit_user_context`] is the only way to turn it into an
//! [`AdmittedContext`], and every service that reads, expands or forwards
//! under a request's context takes the admitted one. An operation that never
//! resolves its context says so with [`SubmittedContext::unused`].
//!
//! Clause 5.5.7's second prohibition (no part of the user `@context` below
//! the top level of the document) is a property of the same user
//! `@context`. The request body extractors find it on the request text and
//! carry it in the [`SubmittedContext`], so the admission refuses it on every
//! operation, after the context loaded and was found free of scoped contexts.
//!
//! A context that passes both is then processed, the last step of the
//! admission: a context that loads but does not process (a malformed term
//! definition, a redefined keyword) is refused here, so every service handed an
//! [`AdmittedContext`] reads under a context that processes, and none of them
//! checks it again. The processed context also says which of its terms stand
//! for core terms, and the [`AdmittedContext`] carries that table, so the
//! request body is read through the terms its own context defines.
//!
//! Three things shape the rule itself:
//!
//! - It is about the request's `@context`, not about its payload, so it reads
//!   a [`ContextSpec`] and never the document.
//! - Deciding it needs the network: a `Link` header context and a body context
//!   written as a URL are documents that have to be fetched before anything can
//!   be said about them, which rules out the synchronous extractor that builds
//!   the [`ContextSpec`].
//! - The Core `@context` is exempt, and not by omission. Its `ngsildproof` term
//!   declares a scoped context naming `https://w3id.org/security/data-integrity/v2`,
//!   which is what keeps `entityIdSealed` and `entityTypeSealed` reachable only
//!   there (clause 4.5.2.2), and clause 5.5.7 addresses the *user* `@context`
//!   throughout, calling the Core one "protected". The W3C Data Integrity
//!   context the Core names is compiled into the broker with it and declares
//!   scoped contexts of its own (one per proof type and proof purpose), so a
//!   user `@context` naming it brings in nothing the Core does not already
//!   bring under `ngsildproof`. Every embedded reference is therefore skipped
//!   by name rather than inspected.
//!
//! The walk resolves each non-core context once and follows the remote
//! references a resolved document itself names, because a scoped context two
//! hops away is as effective as one written inline. Resolution goes through the
//! engine's scanned-context cache, so the verdict for a URL is reached once for
//! the life of the engine and every later request reads it from memory.
//!
//! A payload a Context Source sent back, a notification this broker rendered
//! and a snapshot clone carry no user `@context` and never pass through here.

use crate::{
    context_graph::check_context_graph,
    context_spec::ContextSpec,
    context_value::{ContextEntry, context_entries},
    embedded_context::is_embedded_context_url,
    engine::{
        processor::JsonLdEngine,
        user_context_scan::{UserContextScan, scan_written_context},
    },
    error::JsonLdError,
};
use andromeda_json_literal::piece::JsonKind;
use andromeda_ngsi_ld_representation::{context_terms::ContextTerms, embedded_context_admission::EmbeddedContext};
use derive_more::Deref;
use iri_rs::IriBuf;
use std::{
    collections::HashSet,
    sync::{Arc, LazyLock},
};

/// The `@context` a client request named, not yet admitted, and the first
/// part of it the request body embedded below a document's top level.
///
/// It exposes nothing of the context it carries: an operation reads under a
/// request's context only once [`admit_user_context`] has admitted it.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a submitted context is admitted, or declared unused"]
pub struct SubmittedContext {
    /// The context the request named, in its body or `Link` header.
    spec: ContextSpec,
    /// The first `@context` the request body carried below a document's top
    /// level (clause 5.5.7), if any.
    embedded: Option<EmbeddedContext>,
}

impl SubmittedContext {
    /// The context a request body named as `spec`, with the first `@context`
    /// the body embedded below a document's top level.
    pub const fn new(spec: ContextSpec, embedded: Option<EmbeddedContext>) -> SubmittedContext {
        SubmittedContext { spec, embedded }
    }

    /// Declare that the operation never resolves the context it was submitted
    /// with: one whose input names no term a context could define.
    ///
    /// # Errors
    ///
    /// Returns the [`EmbeddedContext`] the body carried: clause 5.5.7 refuses
    /// it whether or not the context is ever resolved.
    pub fn unused(self) -> Result<(), EmbeddedContext> {
        self.embedded.map_or(Ok(()), Err)
    }
}

// A manual impl because the derive would take both fields: a context named
// without a body has no part of it embedded anywhere.
impl From<ContextSpec> for SubmittedContext {
    fn from(spec: ContextSpec) -> SubmittedContext {
        SubmittedContext { spec, embedded: None }
    }
}

/// The terms of the Core `@context`: the clause 4.4 keyword spellings alone,
/// since the Core `@context` defines each core term under its own name, and no
/// container.
static CORE_TERMS: LazyLock<Arc<ContextTerms>> = LazyLock::new(|| Arc::new(ContextTerms::keyword_spellings().clone())); // clone: once, into the handle every core-only admission shares

/// A user `@context` [`admit_user_context`] admitted: every context it names
/// loaded, none declares a JSON-LD Scoped Context, and the whole processes.
#[derive(Debug, Clone, Deref)]
pub struct AdmittedContext {
    /// The admitted context.
    #[deref]
    spec: ContextSpec,
    /// The terms the processed context defines for core terms and the terms
    /// whose container reshapes what is written under them, which the request
    /// body is read through (clause 5.5.4: "according to the @context").
    terms: Arc<ContextTerms>,
}

impl AdmittedContext {
    /// The Core `@context` alone, which a document the broker writes itself is
    /// read under: clause 5.5.7 addresses the *user* `@context` and calls the
    /// Core one protected, so there is nothing in it to admit.
    #[must_use]
    pub fn core_only() -> AdmittedContext {
        AdmittedContext {
            spec: ContextSpec::core_only(),
            terms: Arc::clone(&CORE_TERMS),
        }
    }

    /// The aliases and containers of the context, shared with the processed
    /// context they were read from.
    #[must_use]
    pub const fn terms(&self) -> &Arc<ContextTerms> {
        &self.terms
    }

    /// The admitted context, for a consumer that keeps it beyond the request,
    /// such as the batch a forwarded deletion names it for.
    #[must_use]
    pub fn into_spec(self) -> ContextSpec {
        self.spec
    }
}

/// Admit the user `@context` a request submitted.
///
/// Every non-core context the request selected is resolved and inspected, in no
/// particular order: the refusal is a property of the set, so a scoped context
/// in the second of two contexts is refused exactly as one in the first.
///
/// # Errors
///
/// Returns, in this order: [`JsonLdError::ContextGraph`] when the contexts'
/// references form a cycle or nest too deep, the typed context-loading variant
/// when one of them does not resolve (clause 5.5.6 answers that with
/// `LdContextNotAvailable`), [`JsonLdError::ScopedUserContext`] naming the
/// first term found carrying a scoped `@context`, and
/// [`JsonLdError::EmbeddedUserContext`] locating the first `@context` the body
/// embedded below a document's top level, and the typed JSON-LD processing
/// failure when the context loaded but does not process.
pub async fn admit_user_context(engine: &JsonLdEngine, submitted: SubmittedContext) -> Result<AdmittedContext, JsonLdError> {
    let SubmittedContext { spec, embedded } = submitted;
    screen_user_context(engine, &spec).await?;
    if let Some(embedded) = embedded {
        return Err(JsonLdError::EmbeddedUserContext(embedded));
    }
    let processed = engine.expansion_context(&spec).await?;
    Ok(AdmittedContext {
        spec,
        terms: Arc::clone(processed.terms()),
    })
}

/// Decide whether `context` is a user `@context` clause 5.5.7 admits, as
/// [`admit_user_context`] does.
///
/// # Errors
///
/// As [`admit_user_context`].
pub(crate) async fn screen_user_context(engine: &JsonLdEngine, context: &ContextSpec) -> Result<(), JsonLdError> {
    // A context whose references cycle or nest too deep is refused before any
    // of its documents is inspected, so no document past the limit is fetched.
    check_context_graph(engine.loader(), &context.resolved_context()).await?;
    let mut pending: Vec<IriBuf> = context.link_context().into_iter().cloned().collect(); // clone: the worklist outlives the borrowed spec
    if let Some(body) = context.body_context() {
        for entry in context_entries(body.piece()) {
            match entry {
                ContextEntry::Reference(url) => {
                    // owned: the worklist outlives the borrowed context value
                    if let Ok(url) = IriBuf::new(url.into_owned()) {
                        pending.push(url);
                    }
                }
                ContextEntry::Definition(definition) => {
                    let scan = scan_written_context(definition.written())?;
                    refuse_scoped_term(&scan)?;
                    pending.extend(scan.nested_references);
                }
                // An array nested in the array is searched all the same; a
                // scalar names no context, and `null` resets one rather than
                // declaring terms.
                ContextEntry::Malformed(nested) if nested.kind() == JsonKind::Array => {
                    let scan = scan_written_context(nested)?;
                    refuse_scoped_term(&scan)?;
                    pending.extend(scan.nested_references);
                }
                ContextEntry::Null | ContextEntry::Malformed(_) => {}
            }
        }
    }

    let mut visited: HashSet<IriBuf> = HashSet::new();
    while let Some(url) = pending.pop() {
        if is_embedded_context_url(url.as_ref()) || visited.contains(&url) {
            continue;
        }
        let scan = engine.user_context_scans().get_or_load(engine.loader(), &url).await?;
        refuse_scoped_term(&scan)?;
        // clone: the worklist owns the references the cached scan keeps
        pending.extend(scan.nested_references.iter().cloned());
        visited.insert(url);
    }
    Ok(())
}

/// Refuse one scanned context document that declares a scoped `@context`.
fn refuse_scoped_term(scan: &UserContextScan) -> Result<(), JsonLdError> {
    match &scan.scoped_context_term {
        Some(term) => Err(JsonLdError::ScopedUserContext {
            term: term.clone(), // clone: the scan is cached and outlives the refusal
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_graph::ContextGraphError,
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        error::JsonLdError,
        test_documents::literal,
        user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
    };
    use andromeda_ngsi_ld_representation::{
        context_terms::ContextTerms,
        embedded_context_admission::{EmbeddedContext, find_embedded_context},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::{
        borrow::Cow,
        collections::HashMap,
        env,
        fs,
        path::PathBuf,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };
    use url::Url;

    /// A process-wide counter making every temporary context file name unique,
    /// which a wall-clock timestamp does not guarantee: sibling tests can write
    /// their first document within the same nanosecond and collide.
    static CONTEXT_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    /// Write one context document to a temporary file, named so parallel tests
    /// cannot collide.
    fn written_context(index: usize, document: &Value) -> PathBuf {
        let sequence = CONTEXT_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("andromeda-scoped-context-{}-{sequence}-{index}.jsonld", process::id()));
        fs::write(&path, document.to_string()).expect("the temporary directory is writable");
        path
    }

    /// An engine that resolves the given context documents and nothing else.
    ///
    /// Preloading is the only way to seed a context that resolves without a
    /// network, so each document is written to a temporary file the builder
    /// reads into the engine's registry; nothing reads the files afterwards, so
    /// they are removed before the engine is handed back.
    fn engine_serving(documents: &[(&str, Value)]) -> JsonLdEngine {
        let mut preloads = HashMap::new();
        let mut paths = Vec::with_capacity(documents.len());
        for (index, (url, document)) in documents.iter().enumerate() {
            let path = written_context(index, document);
            preloads.insert(Url::parse(url).expect("the fixture names a valid URL"), path.clone());
            paths.push(path);
        }
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");
        for path in paths {
            fs::remove_file(&path).expect("the temporary context is removable");
        }
        engine
    }

    /// Admit `context` as a request would submit it.
    async fn admitted(engine: &JsonLdEngine, context: &ContextSpec) -> Result<AdmittedContext, JsonLdError> {
        admit_user_context(engine, SubmittedContext::from(context.clone())).await
    }

    fn link_spec(url: &str) -> ContextSpec {
        ContextSpec::from_link(IriBuf::new(url.to_owned()).expect("a valid IRI"))
    }

    /// The term a refusal names, so a test asserts on the term rather than on
    /// the rendered message.
    fn refused_term(error: &JsonLdError) -> &str {
        let JsonLdError::ScopedUserContext { term } = error else {
            panic!("expected a scoped-context refusal, got {error}");
        };
        term.as_str()
    }

    #[tokio::test]
    async fn a_context_declaring_no_scoped_context_is_admitted() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!({"speed": "https://example.org/speed"})));
        assert!(admitted(&engine, &context).await.is_ok());
    }

    #[tokio::test]
    async fn an_inline_body_context_declaring_a_scoped_context_is_refused() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!({
            "speed": {"@id": "https://example.org/speed", "@context": {"unitCode": "https://example.org/rewritten"}}
        })));
        let error = admitted(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "speed");
    }

    #[tokio::test]
    async fn a_link_header_context_declaring_a_scoped_context_is_refused() {
        let engine = engine_serving(&[(
            "https://example.org/scoped.jsonld",
            json!({"@context": {"speed": {"@id": "https://example.org/speed", "@context": {"unitCode": "https://example.org/rewritten"}}}}),
        )]);
        let error = admitted(&engine, &link_spec("https://example.org/scoped.jsonld"))
            .await
            .expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "speed");
    }

    /// The refusal is a property of the whole set of contexts, so it cannot
    /// depend on the order they are inspected in.
    #[tokio::test]
    async fn a_scoped_context_in_the_second_of_two_contexts_is_refused() {
        let engine = engine_serving(&[
            ("https://example.org/plain.jsonld", json!({"@context": {"speed": "https://example.org/speed"}})),
            (
                "https://example.org/scoped.jsonld",
                json!({"@context": {"gear": {"@id": "https://example.org/gear", "@context": {"observedAt": "https://example.org/rewritten"}}}}),
            ),
        ]);
        let context = ContextSpec::from_body(literal(&json!(["https://example.org/plain.jsonld", "https://example.org/scoped.jsonld"])));
        let error = admitted(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// An unresolvable neighbour must not hide a scoped context declared
    /// elsewhere in the same request.
    #[tokio::test]
    async fn an_unresolvable_neighbour_does_not_hide_a_scoped_context() {
        let engine = engine_serving(&[(
            "https://example.org/scoped.jsonld",
            json!({"@context": {"gear": {"@id": "https://example.org/gear", "@context": {}}}}),
        )]);
        let context = ContextSpec::from_body(literal(&json!(["https://example.org/absent.jsonld", "https://example.org/scoped.jsonld"])));
        let error = admitted(&engine, &context).await.expect_err("one of the two is refused");
        assert!(
            matches!(error, JsonLdError::ScopedUserContext { .. }) || error.is_context_not_available(),
            "expected either refusal, got {error}"
        );
    }

    /// A context document that names another remote context is followed: a
    /// scoped context one hop away modifies terms exactly as an inline one does.
    #[tokio::test]
    async fn a_scoped_context_reached_through_a_nested_remote_reference_is_refused() {
        let engine = engine_serving(&[
            (
                "https://example.org/suite.jsonld",
                json!({"@context": ["https://example.org/scoped.jsonld", {"speed": "https://example.org/speed"}]}),
            ),
            (
                "https://example.org/scoped.jsonld",
                json!({"@context": {"gear": {"@id": "https://example.org/gear", "@context": {}}}}),
            ),
        ]);
        let error = admitted(&engine, &link_spec("https://example.org/suite.jsonld"))
            .await
            .expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// JSON-LD 1.1 section 4.1.10 merges an `@import`ed context into the
    /// importing definition, scoped term definitions included, so an inline
    /// body context importing a scoped one is refused like the scoped one.
    #[tokio::test]
    async fn an_inline_body_context_importing_a_scoped_context_is_refused() {
        let engine = engine_serving(&[(
            "https://example.org/scoped.jsonld",
            json!({"@context": {"gear": {"@id": "https://example.org/gear", "@context": {}}}}),
        )]);
        let context = ContextSpec::from_body(literal(
            &json!({"@import": "https://example.org/scoped.jsonld", "speed": "https://example.org/speed"}),
        ));
        let error = admitted(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// The same `@import` written inside a remote document one hop away.
    #[tokio::test]
    async fn a_link_header_context_importing_a_scoped_context_is_refused() {
        let engine = engine_serving(&[
            (
                "https://example.org/importing.jsonld",
                json!({"@context": {"@import": "https://example.org/scoped.jsonld", "speed": "https://example.org/speed"}}),
            ),
            (
                "https://example.org/scoped.jsonld",
                json!({"@context": {"gear": {"@id": "https://example.org/gear", "@context": {}}}}),
            ),
        ]);
        let error = admitted(&engine, &link_spec("https://example.org/importing.jsonld"))
            .await
            .expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// An `@import` of the Core `@context` stays exempt: the reference is
    /// followed, and every core reference is skipped by name.
    #[tokio::test]
    async fn an_import_of_the_core_context_is_admitted() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(
            &json!({"@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "speed": "https://example.org/speed"}),
        ));
        assert!(admitted(&engine, &context).await.is_ok());
    }

    /// A context whose references form a cycle is an invalid `@context`
    /// (clause 5.5.6), refused before any of its documents is inspected.
    #[tokio::test]
    async fn a_cycle_of_remote_references_is_refused() {
        let engine = engine_serving(&[
            ("https://example.org/a.jsonld", json!({"@context": ["https://example.org/b.jsonld"]})),
            ("https://example.org/b.jsonld", json!({"@context": ["https://example.org/a.jsonld"]})),
        ]);
        let error = admitted(&engine, &link_spec("https://example.org/a.jsonld"))
            .await
            .expect_err("a cycle is refused");
        assert!(matches!(error, JsonLdError::ContextGraph(ContextGraphError::Cycle(_))), "{error}");
    }

    /// Clause 5.5.7 calls the Core `@context` protected and addresses the rule
    /// to the *user* `@context`. The core document's `ngsildproof` term carries
    /// a scoped `@context` of its own, so a reference to it must not be
    /// inspected under this rule.
    #[tokio::test]
    async fn the_core_context_is_never_inspected_under_this_rule() {
        let engine = engine_serving(&[]);
        for url in ["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()] {
            assert!(admitted(&engine, &link_spec(url)).await.is_ok(), "{url}");
            let body = ContextSpec::from_body(literal(&json!(url)));
            assert!(admitted(&engine, &body).await.is_ok(), "{url}");
        }
    }

    /// A body `@context` array mixes URLs and inline objects, and every element
    /// of it is a user context.
    #[tokio::test]
    async fn every_element_of_a_body_context_array_is_inspected() {
        let engine = engine_serving(&[(
            "https://example.org/scoped.jsonld",
            json!({"@context": {"gear": {"@id": "https://example.org/gear", "@context": {}}}}),
        )]);
        let context = ContextSpec::from_body(literal(&json!([
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld",
            {"speed": "https://example.org/speed"},
            "https://example.org/scoped.jsonld"
        ])));
        let error = admitted(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// A term definition written as an array is not valid JSON-LD, and a
    /// scoped context hidden inside one is still seen.
    #[tokio::test]
    async fn a_scoped_context_inside_an_array_term_declaration_is_refused() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!({
            "speed": [{"@id": "https://example.org/speed", "@context": {}}]
        })));
        let error = admitted(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "speed");
    }

    /// A spec carrying nothing of the request's own resolves to the core
    /// context alone, which this rule never speaks about.
    #[tokio::test]
    async fn a_request_that_supplied_no_context_of_its_own_is_admitted() {
        let engine = engine_serving(&[]);
        assert!(admitted(&engine, &ContextSpec::none()).await.is_ok());
        assert!(admitted(&engine, &ContextSpec::core_only()).await.is_ok());
    }

    /// What an operation reads under is the context the request submitted.
    #[tokio::test]
    async fn an_admitted_context_is_the_one_submitted() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!({"speed": "https://example.org/speed"})));
        let admitted = admitted(&engine, &context).await.expect("admitted");
        pretty_assertions::assert_eq!(&*admitted, &context);
    }

    /// The body is read "according to the @context" (clause 5.5.4), so the
    /// admission carries the terms the processed context defines for core
    /// terms.
    #[tokio::test]
    async fn an_admitted_context_carries_the_aliases_of_its_processed_context() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!({"kind": "@type", "v": "https://uri.etsi.org/ngsi-ld/hasValue"})));
        let admitted = admitted(&engine, &context).await.expect("admitted");
        pretty_assertions::assert_eq!(admitted.terms().aliases().core_name(Cow::Borrowed("kind")), "type");
        pretty_assertions::assert_eq!(admitted.terms().aliases().core_name(Cow::Borrowed("v")), "value");
        pretty_assertions::assert_eq!(admitted.terms().aliases().core_name(Cow::Borrowed("@id")), "id");
        pretty_assertions::assert_eq!(&**AdmittedContext::core_only().terms(), ContextTerms::keyword_spellings());
    }

    /// Clause 5.5.4 judges data "according to the @context", so a context that
    /// cannot be retrieved is answered as such before anything clause 5.5.7
    /// refuses in a neighbour is looked for.
    #[tokio::test]
    async fn an_unloadable_context_is_refused_before_a_scoped_neighbour() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!([
            "https://example.org/absent.jsonld",
            {"speed": {"@id": "https://example.org/speed", "@context": {}}}
        ])));
        let error = admitted(&engine, &context).await.expect_err("the absent context is refused");
        assert!(error.is_context_not_available(), "expected a loading error, got {error}");
    }

    /// A context that loads and declares no scoped context, but whose term
    /// definition is a number, is refused at admission rather than by each
    /// service reading under it.
    #[tokio::test]
    async fn a_context_that_loads_but_does_not_process_is_refused() {
        let engine = engine_serving(&[("https://example.org/invalid.jsonld", json!({"@context": {"speed": 42}}))]);
        for context in [
            ContextSpec::from_body(literal(&json!({"speed": 42}))),
            link_spec("https://example.org/invalid.jsonld"),
        ] {
            let error = admitted(&engine, &context).await.expect_err("the context does not process");
            assert!(!error.is_context_not_available(), "expected a processing failure, got {error}");
            assert!(!matches!(error, JsonLdError::ScopedUserContext { .. }), "{error}");
        }
    }

    /// A finding locating `@context` under `/speed`, as the extractor makes it.
    fn embedded_under_speed() -> EmbeddedContext {
        find_embedded_context(br#"{"speed": {"type": "Property", "value": 1, "@context": {}}}"#)
            .expect("valid JSON")
            .expect("an embedded context")
    }

    /// The admission of `context` submitted with the embedded finding.
    async fn admitted_with_finding(engine: &JsonLdEngine, context: &ContextSpec) -> Result<AdmittedContext, JsonLdError> {
        admit_user_context(engine, SubmittedContext::new(context.clone(), Some(embedded_under_speed()))).await
    }

    /// Clause 5.5.7: a loadable context submitted with a `@context` embedded
    /// below the top level is refused for the embedding, located.
    #[tokio::test]
    async fn an_embedded_context_is_refused_once_the_context_loaded() {
        let engine = engine_serving(&[]);
        let error = admitted_with_finding(&engine, &ContextSpec::from_body(literal(&json!({"speed": "https://example.org/speed"}))))
            .await
            .expect_err("clause 5.5.7 refuses it");
        let JsonLdError::EmbeddedUserContext(found) = error else {
            panic!("expected the embedded refusal, got {error}");
        };
        pretty_assertions::assert_eq!(found.path.to_string(), "/speed");
    }

    /// Clause 5.5.4 judges data "according to the @context": a context that
    /// cannot be retrieved is answered as such before the embedding is.
    #[tokio::test]
    async fn an_unloadable_context_is_refused_before_an_embedded_one() {
        let engine = engine_serving(&[]);
        let error = admitted_with_finding(&engine, &link_spec("https://example.org/absent.jsonld"))
            .await
            .expect_err("the absent context is refused");
        assert!(error.is_context_not_available(), "expected a loading error, got {error}");
    }

    #[tokio::test]
    async fn a_scoped_context_is_refused_before_an_embedded_one() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(literal(&json!({"speed": {"@id": "https://example.org/speed", "@context": {}}})));
        let error = admitted_with_finding(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "speed");
    }

    /// An operation that never resolves its context still refuses an embedded
    /// one.
    #[test]
    fn an_unused_context_refuses_only_an_embedded_one() {
        pretty_assertions::assert_eq!(
            SubmittedContext::new(ContextSpec::none(), Some(embedded_under_speed())).unused(),
            Err(embedded_under_speed())
        );
        pretty_assertions::assert_eq!(SubmittedContext::new(ContextSpec::none(), None).unused(), Ok(()));
    }
}
