//! Whether Add @context stores only a `@context` that is valid JSON-LD
//! (clause 5.13.2.4, applying the clause 5.5.4 validation).
//!
//! Driven through the service over an in-memory store, so a refusal is seen to
//! store nothing.

use crate::{
    error::ContextRegistryError,
    service::memory_store::{BrokerRoot, MemoryStore, service},
};
use andromeda_json_literal::JsonLiteral;
use std::sync::Arc;

fn literal(document: &str) -> JsonLiteral {
    JsonLiteral::admit(document).expect("an admissible document")
}

#[tokio::test]
async fn an_invalid_context_is_refused_and_nothing_is_stored() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    for document in [
        r#"{"@context": {"speed": 5}}"#,
        r#"{"@context": {"speed": true}}"#,
        r#"{"@context": "not a url"}"#,
        r#"{"@context": {"@type": "https://example.org/t"}}"#,
        r#"{"@context": {"speed": {"@id": 5}}}"#,
        r#"{"@context": {"a": "b:x", "b": "a:y"}}"#,
        r#"{"@context": {"speed": {"@id": "https://example.org/speed", "@container": "@bogus"}}}"#,
    ] {
        let refusal = registry
            .create(&literal(document), &BrokerRoot)
            .await
            .expect_err("an invalid @context is refused");
        assert!(matches!(refusal, ContextRegistryError::InvalidContext(_)), "{document}: {refusal}");
    }
    pretty_assertions::assert_eq!(store.len(), 0);
}

#[tokio::test]
async fn a_refusal_names_the_term_and_what_a_term_definition_may_be() {
    let store = Arc::new(MemoryStore::default());
    let refusal = service(&store)
        .create(&literal(r#"{"@context": {"speed": 5}}"#), &BrokerRoot)
        .await
        .expect_err("an invalid @context is refused");
    pretty_assertions::assert_eq!(
        refusal.to_string(),
        "the @context defines the term speed as a number, but a term definition is null, a string or an object"
    );
}

/// Storing a `@context` retrieves nothing, so one that names a remote context
/// is stored as written.
#[tokio::test]
async fn a_valid_context_and_one_naming_a_remote_context_are_stored() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    for document in [
        r#"{"@context": {"speed": "https://example.org/speed"}}"#,
        r#"{"@context": "https://example.org/remote.jsonld"}"#,
    ] {
        registry.create(&literal(document), &BrokerRoot).await.expect("stored");
    }
    pretty_assertions::assert_eq!(store.len(), 2);
}
