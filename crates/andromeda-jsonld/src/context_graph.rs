//! Refusing a `@context` whose references form a cycle or nest too deep.
//!
//! JSON-LD 1.1 API section 4.1.2 processes a remote context by recursing into
//! it (step 5.2.6), skips a context already processed only while a scoped
//! context is validated (step 5.2.2), and bounds the recursion only by a
//! processor-defined limit on remote contexts (step 5.2.3, the `context
//! overflow` error). A cycle of references is therefore processed again and
//! again until that limit is reached, and the processor's recursion can
//! exhaust a worker thread's stack well before it does. Clause 5.5.6 makes a
//! remote `@context` that is invalid a `BadRequestData`, and a context that
//! overflows is invalid, so the broker walks the reference graph itself before
//! every call into the processor and refuses a cycle, or a chain deeper than
//! [`MAX_CONTEXT_REFERENCE_DEPTH`], without ever handing it on.
//!
//! The walk follows what context processing follows through documents: a
//! string entry of a `@context` slot (step 5.2) and an `@import` value (step
//! 5.6). It loads every document through the loader's own pipeline, so the
//! processor then finds each one already in the in-process cache, and it is
//! iterative, so the walk itself holds no stack per level.

use crate::{error::JsonLdError, loader::http::HttpContextLoader};
use iri_rs::{IriBuf, IriRef};
use std::vec::IntoIter;
use thiserror::Error;

/// The deepest chain of context documents one `@context` may reach through
/// its references, counting the document the `@context` itself names as the
/// first.
///
/// JSON-LD 1.1 API section 4.1.2 step 5.2.3 leaves the limit to the processor.
/// Eight is well beyond any context a data model publishes (a user context
/// importing a domain context importing a shared one is three) and keeps the
/// processor's recursion comfortably inside a 2 MiB worker stack in a debug
/// build, which a unit test proves at exactly this depth. The same bound stops
/// the walk that reads what a context leaves in force last
/// ([`crate::presented_context`]).
pub const MAX_CONTEXT_REFERENCE_DEPTH: usize = 8;

/// Why a `@context`'s reference graph is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextGraphError {
    /// The document at this URL is reached again through the documents it
    /// references.
    #[error("the @context {0} references itself through the documents it references")]
    Cycle(IriBuf),
    /// The document at this URL lies deeper than
    /// [`MAX_CONTEXT_REFERENCE_DEPTH`] references.
    #[error("the @context {0} lies more than {MAX_CONTEXT_REFERENCE_DEPTH} references deep")]
    TooDeep(IriBuf),
    /// The JSON-LD processor found the references cycle (`recursive context
    /// inclusion`) or exceed its own limit (`context overflow`, JSON-LD 1.1
    /// API section 4.1.2 step 5.2.3), which the walk above keeps it from ever
    /// reaching on the documents it walks.
    #[error("the JSON-LD processor found the @context references overflowing its limit")]
    ProcessorOverflow,
}

/// One document on the path the walk is following.
struct PathEntry {
    /// The URL the document was reached by.
    url: IriBuf,
    /// The references of the document not yet followed.
    pending: IntoIter<IriBuf>,
}

/// Refuse `context`, the `@context` a document is about to be processed
/// under, when its references form a cycle or nest deeper than
/// [`MAX_CONTEXT_REFERENCE_DEPTH`].
///
/// A relative reference in `context` itself has no base to resolve against
/// and is left for the processor to refuse.
///
/// # Errors
///
/// Returns [`JsonLdError::ContextGraph`] for a cycle or a chain too deep, and
/// the failure a referenced document cannot be loaded with, exactly as
/// processing would have reported it.
pub(crate) async fn check_context_graph(loader: &HttpContextLoader, context: &serde_json::Value) -> Result<(), JsonLdError> {
    let mut roots = Vec::new();
    collect_json_slot(context, &mut roots);
    walk(loader, roots).await
}

/// [`check_context_graph`] for the `@context` of `document`, and of each
/// member of an array document.
///
/// # Errors
///
/// As [`check_context_graph`].
pub(crate) async fn check_document_contexts(loader: &HttpContextLoader, document: &serde_json::Value) -> Result<(), JsonLdError> {
    let mut roots = Vec::new();
    match document {
        serde_json::Value::Object(members) => collect_json_member(members, &mut roots),
        serde_json::Value::Array(items) => items.iter().for_each(|item| {
            if let serde_json::Value::Object(members) = item {
                collect_json_member(members, &mut roots);
            }
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {}
    }
    walk(loader, roots).await
}

/// [`check_document_contexts`] for a document already in the processor's
/// JSON form.
///
/// # Errors
///
/// As [`check_context_graph`].
pub(crate) async fn check_syntax_document_contexts(loader: &HttpContextLoader, document: &jsonld_syntax::Value) -> Result<(), JsonLdError> {
    let mut roots = Vec::new();
    match document {
        jsonld_syntax::Value::Object(_) => collect_syntax_member(document, None, &mut roots),
        jsonld_syntax::Value::Array(items) => items.iter().for_each(|item| collect_syntax_member(item, None, &mut roots)),
        jsonld_syntax::Value::Null | jsonld_syntax::Value::Boolean(_) | jsonld_syntax::Value::Number(_) | jsonld_syntax::Value::String(_) => {}
    }
    walk(loader, roots).await
}

/// Follow every reference reachable from `roots`, depth first, loading each
/// document through `loader`.
async fn walk(loader: &HttpContextLoader, roots: Vec<IriBuf>) -> Result<(), JsonLdError> {
    let mut roots = roots.into_iter();
    let mut path: Vec<PathEntry> = Vec::new();
    loop {
        let next = match path.last_mut() {
            Some(entry) => entry.pending.next(),
            None => roots.next(),
        };
        let Some(url) = next else {
            if path.pop().is_none() {
                return Ok(());
            }
            continue;
        };
        if path.iter().any(|entry| entry.url == url) {
            return Err(ContextGraphError::Cycle(url).into());
        }
        if path.len() == MAX_CONTEXT_REFERENCE_DEPTH {
            return Err(ContextGraphError::TooDeep(url).into());
        }
        let document = loader.load_value(&url).await?;
        let mut pending = Vec::new();
        collect_syntax_member(&document, Some(&url), &mut pending);
        path.push(PathEntry {
            url,
            pending: pending.into_iter(),
        });
    }
}

/// Collect the absolute references of the `@context` member of `members`.
fn collect_json_member(members: &serde_json::Map<String, serde_json::Value>, references: &mut Vec<IriBuf>) {
    if let Some(slot) = members.get(CONTEXT) {
        collect_json_slot(slot, references);
    }
}

/// Collect the absolute references of one `@context` slot written as JSON.
fn collect_json_slot(slot: &serde_json::Value, references: &mut Vec<IriBuf>) {
    match slot {
        serde_json::Value::String(reference) => references.extend(reference_iri(reference, None)),
        serde_json::Value::Array(entries) => entries.iter().for_each(|entry| collect_json_slot(entry, references)),
        serde_json::Value::Object(definition) => {
            if let Some(serde_json::Value::String(import)) = definition.get(IMPORT) {
                references.extend(reference_iri(import, None));
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

/// Collect the references of the `@context` member of `document`, each
/// resolved against `base`, the URL the processor resolves them against, when
/// the document was loaded from one.
fn collect_syntax_member(document: &jsonld_syntax::Value, base: Option<&IriBuf>, references: &mut Vec<IriBuf>) {
    if let jsonld_syntax::Value::Object(members) = document {
        members.get(CONTEXT).for_each(|slot| collect_syntax_slot(slot, base, references));
    }
}

/// Collect the references of one `@context` slot in the processor's JSON form.
fn collect_syntax_slot(slot: &jsonld_syntax::Value, base: Option<&IriBuf>, references: &mut Vec<IriBuf>) {
    match slot {
        jsonld_syntax::Value::String(reference) => references.extend(reference_iri(reference.as_str(), base)),
        jsonld_syntax::Value::Array(entries) => entries.iter().for_each(|entry| collect_syntax_slot(entry, base, references)),
        jsonld_syntax::Value::Object(definition) => definition.get(IMPORT).for_each(|import| match import {
            jsonld_syntax::Value::String(import) => references.extend(reference_iri(import.as_str(), base)),
            jsonld_syntax::Value::Null
            | jsonld_syntax::Value::Boolean(_)
            | jsonld_syntax::Value::Number(_)
            | jsonld_syntax::Value::Array(_)
            | jsonld_syntax::Value::Object(_) => {}
        }),
        jsonld_syntax::Value::Null | jsonld_syntax::Value::Boolean(_) | jsonld_syntax::Value::Number(_) => {}
    }
}

/// The keyword a context slot is named by.
const CONTEXT: &str = "@context";

/// The keyword JSON-LD 1.1 section 4.1.10 imports a context through.
const IMPORT: &str = "@import";

/// The IRI `reference` names: itself when absolute, else its resolution
/// against `base`. A reference that is no IRI reference, or is relative with
/// no base, names nothing the walk can follow, and the processor refuses it.
fn reference_iri(reference: &str, base: Option<&IriBuf>) -> Option<IriBuf> {
    let Ok(parsed) = IriRef::parse(reference) else {
        return None;
    };
    let mut absolute = String::with_capacity(reference.len());
    match (parsed.is_absolute(), base) {
        (true, Some(_) | None) => absolute.push_str(reference),
        (false, Some(base)) => {
            if parsed.resolve_into(base, &mut absolute).is_err() {
                return None;
            }
        }
        (false, None) => return None,
    }
    let Ok(iri) = IriBuf::new(absolute) else {
        return None;
    };
    Some(iri)
}

#[cfg(test)]
mod tests {
    use crate::{
        context_graph::{ContextGraphError, MAX_CONTEXT_REFERENCE_DEPTH},
        context_spec::ContextSpec,
        engine::{expanded_input::ExpandedInput, options::CompactOptions, preloaded_engine::engine_preloading, processor::JsonLdEngine},
        error::JsonLdError,
        loader::test_origin::{TestOrigin, ok},
    };
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::thread;
    use tokio::runtime::Builder;

    /// The stack a Tokio worker thread gets by default.
    const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;

    fn iri(text: &str) -> IriBuf {
        IriBuf::new(text.to_owned()).expect("a valid IRI")
    }

    /// The URL of the `index`th document of a preloaded chain.
    fn chain_url(index: usize) -> String {
        format!("https://example.org/chain/c{index}.jsonld")
    }

    /// A chain of `length` documents, each defining one term and naming the
    /// next, the last defining `speed`.
    fn chain(length: usize) -> Vec<(String, Value)> {
        (1..=length)
            .map(|index| {
                let document = if index == length {
                    json!({ "@context": { "speed": "https://example.org/speed" } })
                } else {
                    json!({ "@context": [{ format!("t{index}"): format!("https://example.org/t{index}") }, chain_url(index + 1)] })
                };
                (chain_url(index), document)
            })
            .collect()
    }

    /// A vehicle carrying one `speed` Property.
    fn vehicle() -> Value {
        json!({ "id": "urn:ngsi-ld:Vehicle:graph", "type": "Vehicle", "speed": { "type": "Property", "value": 1 } })
    }

    /// Expand the vehicle under the one context `url` names.
    async fn expanded_under(engine: &JsonLdEngine, url: &str) -> Result<Value, JsonLdError> {
        engine.expand(vehicle(), &ContextSpec::from_link(iri(url))).await
    }

    /// A chain of exactly [`MAX_CONTEXT_REFERENCE_DEPTH`] documents is
    /// processed, and the processor's recursion through it fits the stack a
    /// Tokio worker thread gets: the limit keeps a legal context from ever
    /// overflowing a worker, even in a debug build.
    #[test]
    fn a_chain_at_the_depth_limit_is_processed_on_a_worker_sized_stack() {
        let engine = engine_preloading(&chain(MAX_CONTEXT_REFERENCE_DEPTH));
        let worker = thread::Builder::new()
            .stack_size(WORKER_STACK_BYTES)
            .spawn(move || {
                let runtime = Builder::new_current_thread().build().expect("a current-thread runtime");
                runtime.block_on(expanded_under(&engine, &chain_url(1)))
            })
            .expect("the worker thread starts");

        let expanded = worker.join().expect("the worker does not overflow its stack").expect("the chain is processed");

        assert!(expanded[0].get("https://example.org/speed").is_some(), "{expanded}");
    }

    /// JSON-LD 1.1 API section 4.1.2 step 5.2.3: a reference that would open
    /// a document one deeper than the limit is refused, and that document is
    /// never fetched.
    #[tokio::test]
    async fn a_chain_one_deeper_than_the_limit_is_refused_before_its_last_document_is_fetched() {
        let origin = TestOrigin::start().await;
        let depth = MAX_CONTEXT_REFERENCE_DEPTH + 1;
        for index in 1..=depth {
            let body = if index == depth {
                json!({ "@context": { "speed": "https://example.org/speed" } })
            } else {
                json!({ "@context": [{ "t": "https://example.org/t" }, format!("c{}.jsonld", index + 1)] })
            };
            origin
                .stage(&format!("/c{index}.jsonld"), ok("application/ld+json", &body.to_string(), &[]))
                .await;
        }
        let engine = JsonLdEngine::builder().allow_remote(true).build().expect("engine builds");

        let refused = expanded_under(&engine, origin.url("/c1.jsonld").as_str())
            .await
            .expect_err("the chain is too deep");

        assert!(
            matches!(&refused, JsonLdError::ContextGraph(graph) if *graph == ContextGraphError::TooDeep(origin.url(&format!("/c{depth}.jsonld")))),
            "{refused}"
        );
        pretty_assertions::assert_eq!(origin.requests(&format!("/c{depth}.jsonld")), 0);
        pretty_assertions::assert_eq!(origin.requests(&format!("/c{MAX_CONTEXT_REFERENCE_DEPTH}.jsonld")), 1);
    }

    /// Two documents naming each other are a cycle, refused as an invalid
    /// `@context` rather than processed until the processor overflows.
    #[tokio::test]
    async fn two_documents_naming_each_other_are_refused_as_a_cycle() {
        let first = "https://example.org/cycle/a.jsonld";
        let second = "https://example.org/cycle/b.jsonld";
        let engine = engine_preloading(&[
            (first.to_owned(), json!({ "@context": [{ "speed": "https://example.org/speed" }, second] })),
            (second.to_owned(), json!({ "@context": first })),
        ]);

        let refused = expanded_under(&engine, first).await.expect_err("a cycle is refused");

        assert!(
            matches!(&refused, JsonLdError::ContextGraph(graph) if *graph == ContextGraphError::Cycle(iri(first))),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_document_naming_itself_is_refused_as_a_cycle() {
        let url = "https://example.org/cycle/self.jsonld";
        let engine = engine_preloading(&[(url.to_owned(), json!({ "@context": [{ "speed": "https://example.org/speed" }, url] }))]);

        let refused = expanded_under(&engine, url).await.expect_err("a self-reference is refused");

        assert!(
            matches!(&refused, JsonLdError::ContextGraph(graph) if *graph == ContextGraphError::Cycle(iri(url))),
            "{refused}"
        );
    }

    /// JSON-LD 1.1 API section 4.1.2 step 5.6: an `@import` opens the document
    /// it names as a reference does, so imports that close a loop are a cycle.
    #[tokio::test]
    async fn imports_that_close_a_loop_are_refused_as_a_cycle() {
        let first = "https://example.org/cycle/import-a.jsonld";
        let second = "https://example.org/cycle/import-b.jsonld";
        let engine = engine_preloading(&[
            (
                first.to_owned(),
                json!({ "@context": { "@import": second, "speed": "https://example.org/speed" } }),
            ),
            (second.to_owned(), json!({ "@context": { "@import": first } })),
        ]);

        let refused = expanded_under(&engine, first).await.expect_err("an import cycle is refused");

        assert!(
            matches!(&refused, JsonLdError::ContextGraph(graph) if *graph == ContextGraphError::Cycle(iri(first))),
            "{refused}"
        );
    }

    /// A cycle closed through relative references is found once each is
    /// resolved against the document it was read from, and each document is
    /// fetched once.
    #[tokio::test]
    async fn a_cycle_through_relative_references_is_refused() {
        let origin = TestOrigin::start().await;
        origin
            .stage(
                "/contexts/cycle-a.jsonld",
                ok(
                    "application/ld+json",
                    r#"{"@context": [{"speed": "https://example.org/speed"}, "cycle-b.jsonld"]}"#,
                    &[],
                ),
            )
            .await;
        origin
            .stage("/contexts/cycle-b.jsonld", ok("application/ld+json", r#"{"@context": "cycle-a.jsonld"}"#, &[]))
            .await;
        let engine = JsonLdEngine::builder().allow_remote(true).build().expect("engine builds");

        let refused = expanded_under(&engine, origin.url("/contexts/cycle-a.jsonld").as_str())
            .await
            .expect_err("a relative cycle is refused");

        assert!(
            matches!(&refused, JsonLdError::ContextGraph(graph) if *graph == ContextGraphError::Cycle(origin.url("/contexts/cycle-a.jsonld"))),
            "{refused}"
        );
        pretty_assertions::assert_eq!(origin.requests("/contexts/cycle-a.jsonld"), 1);
        pretty_assertions::assert_eq!(origin.requests("/contexts/cycle-b.jsonld"), 1);
    }

    /// The walk loads each document through the loader's pipeline, so the
    /// processor finds it already held: checking a context costs no fetch of
    /// its own.
    #[tokio::test]
    async fn checking_a_context_fetches_no_document_twice() {
        let origin = TestOrigin::start().await;
        origin
            .stage(
                "/wrapper.jsonld",
                ok("application/ld+json", r#"{"@context": ["terms.jsonld", {"@import": "shared.jsonld"}]}"#, &[]),
            )
            .await;
        origin
            .stage(
                "/terms.jsonld",
                ok("application/ld+json", r#"{"@context": {"speed": "https://example.org/speed"}}"#, &[]),
            )
            .await;
        origin
            .stage(
                "/shared.jsonld",
                ok("application/ld+json", r#"{"@context": {"brand": "https://example.org/brand"}}"#, &[]),
            )
            .await;
        let engine = JsonLdEngine::builder().allow_remote(true).build().expect("engine builds");

        for _ in 0..2 {
            let expanded = expanded_under(&engine, origin.url("/wrapper.jsonld").as_str())
                .await
                .expect("the context is processed");
            assert!(expanded[0].get("https://example.org/speed").is_some(), "{expanded}");
        }

        for path in ["/wrapper.jsonld", "/terms.jsonld", "/shared.jsonld"] {
            pretty_assertions::assert_eq!(origin.requests(path), 1, "{path}");
        }
    }

    /// Compaction processes the request context too, and refuses a cycle the
    /// same way.
    #[tokio::test]
    async fn compaction_under_a_cyclic_context_is_refused() {
        let url = "https://example.org/cycle/compact.jsonld";
        let engine = engine_preloading(&[(url.to_owned(), json!({ "@context": [{ "speed": "https://example.org/speed" }, url] }))]);
        let context = ContextSpec::from_link(iri(url));

        let refused = engine
            .compactor(&context)
            .compact(ExpandedInput::Assembled(json!([])), CompactOptions::default())
            .await
            .expect_err("a cycle is refused");

        assert!(
            matches!(&refused, JsonLdError::ContextGraph(graph) if *graph == ContextGraphError::Cycle(iri(url))),
            "{refused}"
        );
    }
}
