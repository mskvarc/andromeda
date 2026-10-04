//! Refusing a user `@context` that declares a JSON-LD Scoped Context.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` shall
//! not "contain JSON-LD Scoped Contexts (see [2], section 4.1.8)", and that
//! "parts of user @context that are not following the two points above should
//! result in an error of type `BadRequestData`, because JSON-LD Scoped Contexts
//! and nested embedded @context could be used to modify terms defined in the
//! Core @context or to reshape NGSI-LD Elements during the expansion of terms".
//! Clauses 4.4 and 4.5.1 restate it.
//!
//! Three things shape where this lives:
//!
//! - The rule is about the request's `@context`, not about its payload, so it
//!   reads a [`ContextSpec`] and never the document.
//! - Deciding it needs the network: a `Link` header context and a body context
//!   written as a URL are documents that have to be fetched before anything can
//!   be said about them, which rules out the synchronous extractor that builds
//!   the [`ContextSpec`].
//! - The Core `@context` is exempt, and not by omission. Its `ngsildproof` term
//!   names `https://w3id.org/security/data-integrity/v2` as a scoped context --
//!   which is what keeps `entityIdSealed` and `entityTypeSealed` reachable only
//!   there (clause 4.5.2.2) -- and clause 5.5.7 addresses the *user* `@context`
//!   throughout, calling the Core one "protected". Every core reference is
//!   therefore skipped by name rather than inspected.
//!
//! The walk resolves each non-core context once and follows the remote
//! references a resolved document itself names, because a scoped context two
//! hops away is as effective as one written inline. Resolution goes through the
//! engine's scanned-context cache, so the verdict for a URL is reached once for
//! the life of the engine and every later request reads it from memory.
//!
//! Nothing here decides which operations run the rule. A payload a Context
//! Source sent back, a notification this broker rendered and a snapshot clone
//! carry no user `@context` and reach no caller of this module.

use crate::{
    context_graph::check_context_graph,
    context_spec::ContextSpec,
    core_context::is_ngsi_ld_core_context_url,
    engine::{
        processor::JsonLdEngine,
        user_context_scan::{UserContextScan, collect_context_items, scan_user_context},
    },
    error::JsonLdError,
};
use iri_rs::IriBuf;
use serde_json::Value;
use std::collections::HashSet;

/// Refuse a request `@context` that declares a JSON-LD Scoped Context anywhere
/// a user context reaches.
///
/// Every non-core context the request selected is resolved and inspected, in no
/// particular order: the refusal is a property of the set, so a scoped context
/// in the second of two contexts is refused exactly as one in the first, and an
/// unresolvable neighbour does not hide it.
///
/// # Errors
///
/// Returns [`JsonLdError::ContextGraph`] when the contexts' references form a
/// cycle or nest too deep, [`JsonLdError::ScopedUserContext`] naming the first term found
/// carrying a scoped `@context`, and the typed context-loading variant when one
/// of the contexts does not resolve -- which clause 5.5.2 answers with
/// `LdContextNotAvailable` rather than with this rule's `BadRequestData`.
pub async fn reject_scoped_user_contexts(engine: &JsonLdEngine, context: &ContextSpec) -> Result<(), JsonLdError> {
    // A context whose references cycle or nest too deep is refused before any
    // of its documents is inspected, so no document past the limit is fetched.
    if let Some(value) = context.to_context_value() {
        check_context_graph(engine.loader(), &value).await?;
    }
    let mut pending: Vec<IriBuf> = context.link_context().into_iter().cloned().collect(); // clone: the worklist outlives the borrowed spec
    if let Some(body) = context.body_context() {
        let mut items = Vec::new();
        collect_context_items(body, &mut items);
        for item in items {
            match item {
                Value::String(url) => {
                    // owned: the worklist outlives the borrowed context value
                    if let Ok(url) = IriBuf::new(url.to_owned()) {
                        pending.push(url);
                    }
                }
                Value::Object(_) => {
                    let scan = scan_user_context(item);
                    refuse_scoped_term(&scan)?;
                    pending.extend(scan.nested_references);
                }
                // `collect_context_items` flattens arrays away; a scalar names
                // no context, and `null` resets one rather than declaring terms.
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => {}
            }
        }
    }

    let mut visited: HashSet<IriBuf> = HashSet::new();
    while let Some(url) = pending.pop() {
        if is_ngsi_ld_core_context_url(url.as_ref()) || visited.contains(&url) {
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
        scoped_context_admission::reject_scoped_user_contexts,
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::{
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
            preloads.insert(Url::parse(url).expect("the fixture names a valid URL"), path.clone()); // clone: the removal below needs the path the preload map takes
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
        let context = ContextSpec::from_body(json!({"speed": "https://example.org/speed"}));
        assert!(reject_scoped_user_contexts(&engine, &context).await.is_ok());
    }

    #[tokio::test]
    async fn an_inline_body_context_declaring_a_scoped_context_is_refused() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(json!({
            "speed": {"@id": "https://example.org/speed", "@context": {"unitCode": "https://example.org/rewritten"}}
        }));
        let error = reject_scoped_user_contexts(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "speed");
    }

    #[tokio::test]
    async fn a_link_header_context_declaring_a_scoped_context_is_refused() {
        let engine = engine_serving(&[(
            "https://example.org/scoped.jsonld",
            json!({"@context": {"speed": {"@id": "https://example.org/speed", "@context": {"unitCode": "https://example.org/rewritten"}}}}),
        )]);
        let error = reject_scoped_user_contexts(&engine, &link_spec("https://example.org/scoped.jsonld"))
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
        let context = ContextSpec::from_body(json!(["https://example.org/plain.jsonld", "https://example.org/scoped.jsonld"]));
        let error = reject_scoped_user_contexts(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
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
        let context = ContextSpec::from_body(json!(["https://example.org/absent.jsonld", "https://example.org/scoped.jsonld"]));
        let error = reject_scoped_user_contexts(&engine, &context).await.expect_err("one of the two is refused");
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
        let error = reject_scoped_user_contexts(&engine, &link_spec("https://example.org/suite.jsonld"))
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
        let context = ContextSpec::from_body(json!({"@import": "https://example.org/scoped.jsonld", "speed": "https://example.org/speed"}));
        let error = reject_scoped_user_contexts(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
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
        let error = reject_scoped_user_contexts(&engine, &link_spec("https://example.org/importing.jsonld"))
            .await
            .expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// An `@import` of the Core `@context` stays exempt: the reference is
    /// followed, and every core reference is skipped by name.
    #[tokio::test]
    async fn an_import_of_the_core_context_is_admitted() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(json!({"@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "speed": "https://example.org/speed"}));
        assert!(reject_scoped_user_contexts(&engine, &context).await.is_ok());
    }

    /// A context whose references form a cycle is an invalid `@context`
    /// (clause 5.5.6), refused before any of its documents is inspected.
    #[tokio::test]
    async fn a_cycle_of_remote_references_is_refused() {
        let engine = engine_serving(&[
            ("https://example.org/a.jsonld", json!({"@context": ["https://example.org/b.jsonld"]})),
            ("https://example.org/b.jsonld", json!({"@context": ["https://example.org/a.jsonld"]})),
        ]);
        let error = reject_scoped_user_contexts(&engine, &link_spec("https://example.org/a.jsonld"))
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
            assert!(reject_scoped_user_contexts(&engine, &link_spec(url)).await.is_ok(), "{url}");
            let body = ContextSpec::from_body(json!(url));
            assert!(reject_scoped_user_contexts(&engine, &body).await.is_ok(), "{url}");
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
        let context = ContextSpec::from_body(json!([
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld",
            {"speed": "https://example.org/speed"},
            "https://example.org/scoped.jsonld"
        ]));
        let error = reject_scoped_user_contexts(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "gear");
    }

    /// A term definition written as an array is not valid JSON-LD, and a
    /// scoped context hidden inside one is still seen.
    #[tokio::test]
    async fn a_scoped_context_inside_an_array_term_declaration_is_refused() {
        let engine = engine_serving(&[]);
        let context = ContextSpec::from_body(json!({
            "speed": [{"@id": "https://example.org/speed", "@context": {}}]
        }));
        let error = reject_scoped_user_contexts(&engine, &context).await.expect_err("clause 5.5.7 refuses it");
        pretty_assertions::assert_eq!(refused_term(&error), "speed");
    }

    /// A spec carrying nothing of the request's own resolves to the core
    /// context alone, which this rule never speaks about.
    #[tokio::test]
    async fn a_request_that_supplied_no_context_of_its_own_is_admitted() {
        let engine = engine_serving(&[]);
        assert!(reject_scoped_user_contexts(&engine, &ContextSpec::none()).await.is_ok());
        assert!(reject_scoped_user_contexts(&engine, &ContextSpec::core_only()).await.is_ok());
    }
}
