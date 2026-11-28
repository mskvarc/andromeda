//! Whether Delete @context with reload replaces a Cached entry only with a
//! download that is valid JSON-LD (clause 5.13.5.4, applying the clause 5.5.4
//! validation).
//!
//! Driven through the service over an in-memory store, with each document
//! served from a local mock origin, so a refused download is seen to leave the
//! entry holding what it held before.

use crate::{
    error::ContextRegistryError,
    identifier::ContextIdentifier,
    removal::ContextRemoval,
    service::memory_store::{MemoryStore, downloading_service},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

/// The document the entry holds before it is reloaded.
const PREVIOUS: &str = r#"{"@context": {"speed": "https://example.org/speed"}}"#;

/// The path the origin serves the reloaded document at.
const SERVED_AT: &str = "/context.jsonld";

/// A Cached entry reloaded from its origin.
struct Reloaded {
    /// The entry's identifier.
    identifier: JsonLdContextId,
    /// The URL the entry was downloaded from.
    url: IriBuf,
    /// What the reload answered.
    answer: Result<(), ContextRegistryError>,
}

/// `document` read as JSON, so documents compare by value.
fn parsed(document: &str) -> Value {
    serde_json::from_str(document).expect("a JSON document")
}

/// The document `store` holds under `identifier`.
fn stored(store: &MemoryStore, identifier: &JsonLdContextId) -> Value {
    store.read(identifier.as_ref(), |row| parsed(row.payload.text())).expect("the entry is held")
}

/// Hold a Cached entry with [`PREVIOUS`], have its origin serve `document`,
/// and reload the entry.
async fn reload(store: &Arc<MemoryStore>, document: &str) -> Reloaded {
    let origin = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SERVED_AT))
        .respond_with(ResponseTemplate::new(200).set_body_raw(document, "application/ld+json"))
        .mount(&origin)
        .await;
    let url = IriBuf::new(format!("{}{SERVED_AT}", origin.uri())).expect("an origin URL is an IRI");
    let identifier = JsonLdContextId::mint().expect("a minted identifier");
    store.hold_download(&identifier, url.clone(), JsonLiteral::admit(PREVIOUS).expect("an admissible document"));
    let answer = downloading_service(store)
        .remove(&ContextIdentifier::from(Identifier::from(identifier.clone())), ContextRemoval::Reload)
        .await;
    Reloaded { identifier, url, answer }
}

#[tokio::test]
async fn an_invalid_download_is_refused_and_the_entry_keeps_its_document() {
    let store = Arc::new(MemoryStore::default());
    for document in [
        r#"{"@context": {"speed": 5}}"#,
        r#"{"@context": {"speed": true}}"#,
        r#"{"@context": "not a url"}"#,
        r#"{"@context": {"@type": "https://example.org/t"}}"#,
        r#"{"@context": {"speed": {"@id": 5}}}"#,
        r#"{"@context": {"a": "b:x", "b": "a:y"}}"#,
        r#"{"@context": {"speed": {"@id": "https://example.org/speed", "@container": "@bogus"}}}"#,
    ] {
        let reloaded = reload(&store, document).await;
        assert!(
            matches!(reloaded.answer, Err(ContextRegistryError::InvalidDownload { .. })),
            "{document}: {:?}",
            reloaded.answer
        );
        pretty_assertions::assert_eq!(stored(&store, &reloaded.identifier), parsed(PREVIOUS), "{document}");
    }
}

#[tokio::test]
async fn a_refused_download_names_its_url_and_what_is_wrong_with_it() {
    let store = Arc::new(MemoryStore::default());
    let reloaded = reload(&store, r#"{"@context": {"speed": 5}}"#).await;
    let refusal = reloaded.answer.expect_err("an invalid download is refused");
    pretty_assertions::assert_eq!(
        refusal.to_string(),
        format!(
            r#"the context downloaded from {} is not valid JSON-LD: the @context defines the term "speed" as a number, but a term definition is null, a string or an object"#,
            reloaded.url
        )
    );
}

/// The check retrieves nothing, so a download naming another document is
/// checked only for its syntax and replaces the entry as a valid one does.
#[tokio::test]
async fn a_valid_download_and_one_naming_another_document_replace_the_entry() {
    let store = Arc::new(MemoryStore::default());
    for document in [
        r#"{"@context": {"speed": "https://example.org/velocity"}}"#,
        r#"{"@context": ["https://example.org/remote.jsonld", {"a": "b:x", "b": "a:y"}]}"#,
    ] {
        let reloaded = reload(&store, document).await;
        assert!(reloaded.answer.is_ok(), "{document}: {:?}", reloaded.answer);
        pretty_assertions::assert_eq!(stored(&store, &reloaded.identifier), parsed(document), "{document}");
    }
}
