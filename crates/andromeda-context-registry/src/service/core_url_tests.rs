//! Every spelling of the core @context URL addresses no stored entry.
//!
//! The embedded core Context is not a stored entry: its sole identifier is its
//! ETSI URL (clause 4.4), so clauses 5.13.4.4 and 5.13.5.4 answer that URL as
//! an identifier that "does not correspond to any existing entry in the
//! @context storage" -- `ResourceNotFound` -- whatever version spelling the
//! request writes and whatever projection or lifecycle command it asks for.

use crate::{
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    identifier::ContextIdentifier,
    removal::ContextRemoval,
    service::memory_store::{MemoryStore, service},
};
use std::sync::Arc;

/// The core @context URLs a request can write: the current version, the
/// unversioned alias and one older version.
const CORE_URLS: [&str; 3] = [
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld",
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld",
];

/// Clause 5.13.4.4: serving a core @context URL, content or `details=true`,
/// answers `ResourceNotFound`.
#[tokio::test]
async fn serving_any_core_context_url_answers_not_found_for_content_and_details() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);

    for url in CORE_URLS {
        for detail_level in [ContextDetailLevel::Summary, ContextDetailLevel::Full] {
            let served = registry.get(&ContextIdentifier::from(url), detail_level).await;
            assert!(
                matches!(&served, Err(ContextRegistryError::NotFound { identifier }) if identifier.to_string() == url),
                "{url} must answer NotFound, got {served:?}"
            );
        }
    }
}

/// Clause 5.13.5.4: deleting or reloading a core @context URL, with the
/// not-found refusal preceding any consideration of kind, answers
/// `ResourceNotFound`.
#[tokio::test]
async fn deleting_or_reloading_any_core_context_url_answers_not_found() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);

    for url in CORE_URLS {
        for removal in [ContextRemoval::Delete, ContextRemoval::Reload] {
            let removed = registry.remove(&ContextIdentifier::from(url), removal).await;
            assert!(
                matches!(&removed, Err(ContextRegistryError::NotFound { identifier }) if identifier.to_string() == url),
                "{url} with {removal:?} must answer NotFound, got {removed:?}"
            );
        }
    }
}
