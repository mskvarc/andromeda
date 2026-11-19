//! Whether a `@context` document a client hands over to be stored holds a
//! valid JSON-LD context.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.13.2.4 applies "the behaviour described in
//! clause 5.5.4 about JSON and JSON-LD validation" to an invalid `@context`,
//! and clause 5.5.4 answers JSON-LD that is not syntactically correct with
//! `BadRequestData`. The document's `@context` is read as the JSON-LD processor
//! reads a context document and, when that needs no other document, processed
//! as the processor processes it, against an empty active context, with the
//! result discarded.
//!
//! A `@context` that names another document, by URL or by `@import` at any
//! depth, is checked only as far as its syntax: processing it needs that
//! document, and storing a `@context` retrieves nothing.

use crate::{
    engine::{
        context_fault_reading::{extraction_failure, processing_fault},
        expansion_context::processing_options,
        processing_warning::LoggedWarnings,
        processor_text::ProcessorTextError,
    },
    loader::error::InvalidContextDocument,
};
use andromeda_json_literal::JsonLiteral;
use jsonld::{
    Context as ActiveContext,
    ExtractContext,
    NoLoader,
    context_processing::{Process, algorithm::requires_loader},
};
use jstrict::parse::parse_str_value;
use rdfx::vocabulary::no_vocabulary_mut;
use thiserror::Error;

/// Why a `@context` document is refused for storage.
#[derive(Debug, Error)]
pub enum ContextDocumentRefusal {
    /// The document holds no valid JSON-LD context.
    #[error(transparent)]
    Invalid(InvalidContextDocument),
    /// The processor's reader refused the document, which the broker admitted
    /// as JSON.
    #[error(transparent)]
    Unreadable(#[from] ProcessorTextError),
}

/// Check that `document`, a JSON object with a top-level `@context` member,
/// holds a valid JSON-LD context.
///
/// # Errors
///
/// Returns [`ContextDocumentRefusal::Invalid`] when the `@context` is not
/// valid JSON-LD, naming the term, keyword or member it is wrong in, and
/// [`ContextDocumentRefusal::Unreadable`] when the processor cannot read the
/// document at all.
pub async fn check_context_document(document: &JsonLiteral) -> Result<(), ContextDocumentRefusal> {
    let value = parse_str_value(document.text()).map_err(ProcessorTextError::from)?;
    let context = value
        .into_ld_context()
        .map_err(|error| ContextDocumentRefusal::Invalid(extraction_failure(&error)))?;
    if requires_loader(&context) {
        return Ok(());
    }
    context
        .process_full(
            no_vocabulary_mut(),
            &ActiveContext::new(None),
            &NoLoader,
            None,
            processing_options(),
            LoggedWarnings,
        )
        .await
        .map(|_| ())
        .map_err(|error| ContextDocumentRefusal::Invalid(InvalidContextDocument::InvalidContext(processing_fault(&error))))
}

#[cfg(test)]
mod tests {
    use crate::{
        context_fault::ContextFault,
        engine::context_document_check::{ContextDocumentRefusal, check_context_document},
        loader::error::InvalidContextDocument,
    };
    use andromeda_json_literal::JsonLiteral;

    async fn checked(document: &str) -> Result<(), ContextDocumentRefusal> {
        check_context_document(&JsonLiteral::admit(document).expect("admissible")).await
    }

    /// The fault a refused document reports.
    async fn fault(document: &str) -> ContextFault {
        match checked(document).await {
            Err(ContextDocumentRefusal::Invalid(InvalidContextDocument::InvalidContext(fault))) => fault,
            other => panic!("expected an invalid @context for {document}, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_valid_context_is_accepted() {
        for document in [
            r#"{"@context": {"speed": "https://example.org/speed"}}"#,
            r#"{"@context": {"speed": {"@id": "https://example.org/speed", "@type": "@id", "@container": "@set"}}}"#,
            r#"{"@context": null}"#,
            r#"{"@context": []}"#,
        ] {
            assert!(checked(document).await.is_ok(), "{document}");
        }
    }

    /// Storing a `@context` retrieves nothing, so one naming another document
    /// is processed no further than its syntax, however its own definitions
    /// would process once that document is read.
    #[tokio::test]
    async fn a_context_naming_another_document_is_checked_only_for_its_syntax() {
        for document in [
            r#"{"@context": "https://example.org/remote.jsonld"}"#,
            r#"{"@context": ["https://example.org/remote.jsonld", {"a": "b:x", "b": "a:y"}]}"#,
            r#"{"@context": {"@import": "https://example.org/remote.jsonld", "speed": "https://example.org/speed"}}"#,
            r#"{"@context": {"speed": {"@id": "https://example.org/speed", "@context": "https://example.org/scoped.jsonld"}}}"#,
        ] {
            assert!(checked(document).await.is_ok(), "{document}");
        }
        assert!(matches!(
            fault(r#"{"@context": ["https://example.org/remote.jsonld", {"speed": 5}]}"#).await,
            ContextFault::TermDefinitionKind { .. }
        ));
    }

    #[tokio::test]
    async fn every_invalid_context_is_refused() {
        for document in [
            r#"{"@context": {"speed": 5}}"#,
            r#"{"@context": {"speed": true}}"#,
            r#"{"@context": "not a url"}"#,
            r#"{"@context": {"@type": "https://example.org/t"}}"#,
            r#"{"@context": {"speed": {"@id": 5}}}"#,
            r#"{"@context": {"a": "b:x", "b": "a:y"}}"#,
            r#"{"@context": {"speed": {"@id": "https://example.org/speed", "@container": "@bogus"}}}"#,
        ] {
            assert!(
                matches!(
                    checked(document).await,
                    Err(ContextDocumentRefusal::Invalid(InvalidContextDocument::InvalidContext(_)))
                ),
                "{document}"
            );
        }
    }

    #[tokio::test]
    async fn a_refusal_reads_as_a_statement_about_the_context() {
        let Err(refusal) = checked(r#"{"@context": {"a": "b:x", "b": "a:y"}}"#).await else {
            panic!("a cyclic IRI mapping is refused");
        };
        pretty_assertions::assert_eq!(refusal.to_string(), "the @context cannot define the term a: cyclic IRI mapping");
    }
}
