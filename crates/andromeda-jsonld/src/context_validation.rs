//! Whether a stored `jsonldContext` reference names a context this broker can
//! use.
//!
//! A subscription's `jsonldContext` (Table 5.2.12-1) and the `jsonldContext`
//! key of a registration's `contextSourceInfo` (clause 4.3.6.6) each name, by
//! URL, the `@context` the broker later renders a notification or forwards a
//! request under. Storing one that turns out to be unreachable, malformed or
//! refused defers the failure to that later moment, where no client is
//! listening; so it is admitted once, at the write, exactly as a request naming
//! it in a `Link` header would be.
//!
//! Clause 4.4 knows two kinds of `@context`, the Core one and the user one, so
//! the context such a reference names is a user `@context`. The broker compacts
//! under it, and clause 5.5.7 forbids a user `@context` the JSON-LD Scoped
//! Contexts that could modify Core terms during that compaction.

use crate::{
    context_spec::ContextSpec,
    engine::processor::JsonLdEngine,
    error::JsonLdError,
    user_context_admission::{SubmittedContext, admit_user_context},
};
use iri_rs::IriBuf;

/// Admit the user `@context` a stored `jsonldContext` reference names.
///
/// # Errors
///
/// As [`admit_user_context`]: the typed context-loading failure when the
/// document cannot be retrieved, which clause 5.5.6 answers with
/// `LdContextNotAvailable`; [`JsonLdError::ScopedUserContext`] when it declares
/// a JSON-LD Scoped Context and the typed JSON-LD processing failure when it is
/// not a valid `@context`, both of which clause 5.5.7 answers with
/// `BadRequestData`.
pub async fn admit_context_reference(engine: &JsonLdEngine, reference: &IriBuf) -> Result<(), JsonLdError> {
    let submitted = SubmittedContext::from(ContextSpec::from_link(reference.clone())); // clone: the spec owns the reference the caller keeps storing
    admit_user_context(engine, submitted).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{context_validation::admit_context_reference, engine::processor::JsonLdEngine, error::JsonLdError};
    use iri_rs::IriBuf;
    use std::{
        collections::HashMap,
        env,
        fs,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };
    use url::Url;

    /// Makes every temporary context file name unique, so engines built by
    /// tests running in parallel never write or remove each other's files.
    static CONTEXT_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn reference(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    /// An engine that resolves `url` to `document` and nothing else.
    fn engine_serving(url: &str, document: &str) -> JsonLdEngine {
        let sequence = CONTEXT_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("andromeda-context-reference-{}-{sequence}.jsonld", process::id()));
        fs::write(&path, document).expect("the temporary directory is writable");
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(HashMap::from([(Url::parse(url).expect("a valid URL"), path.clone())]))
            .build()
            .expect("engine builds");
        fs::remove_file(&path).expect("the temporary context is removable");
        engine
    }

    #[tokio::test]
    async fn a_reference_to_a_valid_context_is_accepted() {
        let engine = engine_serving("https://example.com/valid.jsonld", r#"{"@context": {"speed": "https://example.com/speed"}}"#);
        let admitted = admit_context_reference(&engine, &reference("https://example.com/valid.jsonld")).await;
        assert!(admitted.is_ok(), "{:?}", admitted.err());
    }

    #[tokio::test]
    async fn a_reference_that_does_not_resolve_is_a_loading_failure() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let error = admit_context_reference(&engine, &reference("https://example.com/absent.jsonld"))
            .await
            .expect_err("nothing resolves offline");
        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }

    #[tokio::test]
    async fn a_reference_to_a_document_that_is_no_context_is_not_a_loading_failure() {
        let engine = engine_serving("https://example.com/invalid-context.jsonld", r#"{"@context": {"speed": 42}}"#);
        let error = admit_context_reference(&engine, &reference("https://example.com/invalid-context.jsonld"))
            .await
            .expect_err("a term defined as a number is no valid context");
        assert!(!error.is_context_not_available(), "expected a processing failure, got {error}");
    }

    /// Clause 5.5.7: a user `@context` shall not contain JSON-LD Scoped
    /// Contexts, and the context a stored reference names is a user one.
    #[tokio::test]
    async fn a_reference_to_a_context_declaring_a_scoped_context_is_refused() {
        let engine = engine_serving(
            "https://example.com/scoped.jsonld",
            r#"{"@context": {"speed": {"@id": "https://example.com/speed", "@context": {"unitCode": "https://example.com/rewritten"}}}}"#,
        );
        let error = admit_context_reference(&engine, &reference("https://example.com/scoped.jsonld"))
            .await
            .expect_err("clause 5.5.7 refuses it");
        assert!(matches!(&error, JsonLdError::ScopedUserContext { term } if term.as_str() == "speed"), "{error}");
    }
}
