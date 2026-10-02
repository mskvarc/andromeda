//! Which identifier a new Context takes (clauses 5.13.1 and 5.13.2.4).
//!
//! Driven through the service over an in-memory store, so the identity policy
//! is exercised without a database: a Hosted entry is fresh every time, and an
//! `ImplicitlyCreated` entry is shared by equal content and never served for
//! different content.

use crate::{
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    fetched_context::FetchedContext,
    hosted_context_locator::HostedContextLocator,
    identifier::ContextIdentifier,
    insertion::ContextInsertion,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    removal::ContextRemoval,
    service::ContextRegistryService,
    store::{ContextRegistryFuture, ContextRegistryStore},
    stored_context::StoredContext,
};
use andromeda_jsonld::{compound_context_host::WrapperUse, context_cache::DownloadedContext, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use serde_json::{Value, json};
use std::sync::{Arc, RwLock};
use url::Url;
use uuid::{Uuid, Version};

/// One stored row: its identifier, kind and document.
struct Row {
    identifier: String,
    kind: ContextKind,
    payload: Value,
}

/// A store holding its rows in memory, keyed by identifier alone.
#[derive(Default)]
struct MemoryStore {
    rows: RwLock<Vec<Row>>,
}

impl MemoryStore {
    /// Hold `payload` under `identifier` as an entry of `kind`, as a client or
    /// an earlier broker run could have left it.
    fn hold(&self, identifier: &JsonLdContextId, kind: ContextKind, payload: Value) {
        self.rows.write().expect("the store lock").push(Row {
            identifier: identifier.to_string(),
            kind,
            payload,
        });
    }

    /// How many rows the store holds.
    fn len(&self) -> usize {
        self.rows.read().expect("the store lock").len()
    }

    /// The row held under `identifier`, projected by `read`.
    fn read<T>(&self, identifier: &str, read: impl FnOnce(&Row) -> T) -> Option<T> {
        self.rows
            .read()
            .expect("the store lock")
            .iter()
            .find(|row| row.identifier == identifier)
            .map(read)
    }
}

impl ContextRegistryStore for MemoryStore {
    fn insert<'a>(
        &'a self,
        identifier: &'a JsonLdContextId,
        payload: &'a Value,
        _serving_url: &'a Url,
        kind: ContextCreationKind,
    ) -> ContextRegistryFuture<'a, ContextInsertion> {
        Box::pin(async move {
            if self.read(identifier.as_ref(), |_| ()).is_some() {
                return Ok(ContextInsertion::Occupied);
            }
            let kind = match kind {
                ContextCreationKind::Hosted => ContextKind::Hosted,
                ContextCreationKind::ImplicitlyCreated => ContextKind::ImplicitlyCreated,
            };
            self.hold(identifier, kind, payload.clone()); // clone: the store keeps its own copy of the document.
            Ok(ContextInsertion::Inserted)
        })
    }

    fn list<'a>(&'a self, _query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ListedContexts> {
        Box::pin(async move { Ok(ListedContexts::Summary(Vec::new())) })
    }

    fn get<'a>(&'a self, identifier: &'a Identifier, _detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
        Box::pin(async move { Ok(self.read(identifier.as_str(), |row| FetchedContext::Payload(row.payload.clone()))) }) // clone: the read hands back an owned document.
    }

    fn record_access<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move { Ok(ContextMutation::Applied) })
    }

    fn lookup<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
        Box::pin(async move {
            Ok(self.read(identifier.as_str(), |row| StoredContext {
                identifier: identifier.clone(), // clone: the lookup hands back an owned identifier.
                kind: row.kind.clone(),         // clone: the lookup hands back an owned kind.
                source_url: None,
            }))
        })
    }

    fn delete<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let mut rows = self.rows.write().expect("the store lock");
            let before = rows.len();
            rows.retain(|row| row.identifier != identifier.as_str());
            Ok(if rows.len() < before {
                ContextMutation::Applied
            } else {
                ContextMutation::Missing
            })
        })
    }

    fn replace_download<'a>(&'a self, _identifier: &'a Identifier, _download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move { Ok(ContextMutation::Missing) })
    }
}

/// A service over `store`, with an engine that never reaches the network.
fn service(store: &Arc<MemoryStore>) -> ContextRegistryService {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
    ContextRegistryService::new(Arc::clone(store) as Arc<dyn ContextRegistryStore>, engine)
}

/// Serves every entry under `https://broker.example/<identifier>`.
struct BrokerRoot;

impl HostedContextLocator for BrokerRoot {
    fn serving_url(&self, identifier: &JsonLdContextId) -> Url {
        let mut url = Url::parse("https://broker.example/").expect("a URL");
        url.set_path(identifier.as_ref());
        url
    }
}

/// The version of the UUID an identifier carries as its local id.
fn uuid_version(identifier: &JsonLdContextId) -> Option<Version> {
    let text = identifier.to_string();
    let local_id = text.strip_prefix("urn:ngsi-ld:JsonLdContext:").expect("the JsonLdContext prefix");
    Uuid::parse_str(local_id).expect("a hyphenated UUID").get_version()
}

/// An `@context` array a subscription could carry.
fn compound(first: &str, second: &str) -> Value {
    json!({ "@context": [first, second] })
}

/// Clause 5.13.2.4: every Add @context is "a new entry", so the same document
/// posted twice is two entries under two random identifiers.
#[tokio::test]
async fn the_same_hosted_document_created_twice_is_two_entries() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    let document = json!({ "@context": { "Spot": "https://e.org/Spot" } });

    let first = registry.create(document.clone(), &BrokerRoot).await.expect("created"); // clone: the same document is created twice.
    let second = registry.create(document, &BrokerRoot).await.expect("created");

    assert_ne!(first, second);
    pretty_assertions::assert_eq!(uuid_version(&first), Some(Version::Random));
    pretty_assertions::assert_eq!(uuid_version(&second), Some(Version::Random));
    pretty_assertions::assert_eq!(store.len(), 2);
}

/// Equal `@context` arrays share one `ImplicitlyCreated` entry under an
/// identifier derived from their content.
#[tokio::test]
async fn equal_compound_contexts_share_one_implicit_entry() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);

    let first = registry
        .create_implicitly(compound("https://e.org/a.jsonld", "https://e.org/b.jsonld"), &BrokerRoot, WrapperUse::Response)
        .await
        .expect("hosted");
    let second = registry
        .create_implicitly(compound("https://e.org/a.jsonld", "https://e.org/b.jsonld"), &BrokerRoot, WrapperUse::Response)
        .await
        .expect("hosted");

    pretty_assertions::assert_eq!(first, second);
    pretty_assertions::assert_eq!(uuid_version(&first), Some(Version::Sha1));
    pretty_assertions::assert_eq!(store.len(), 1);
}

/// Formatting and member order are not content: the same inline context
/// written with its terms in another order derives the same identifier.
#[tokio::test]
async fn member_order_does_not_split_an_implicit_entry() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    let forward: Value = serde_json::from_str(r#"{"@context": [{"a": "https://e.org/a", "b": "https://e.org/b"}, "https://e.org/c.jsonld"]}"#).expect("JSON");
    let backward: Value = serde_json::from_str(r#"{"@context":[{"b":"https://e.org/b","a":"https://e.org/a"},"https://e.org/c.jsonld"]}"#).expect("JSON");

    let first = registry.create_implicitly(forward, &BrokerRoot, WrapperUse::Response).await.expect("hosted");
    let second = registry.create_implicitly(backward, &BrokerRoot, WrapperUse::Response).await.expect("hosted");

    pretty_assertions::assert_eq!(first, second);
    pretty_assertions::assert_eq!(store.len(), 1);
}

#[tokio::test]
async fn different_compound_contexts_are_different_implicit_entries() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);

    let first = registry
        .create_implicitly(compound("https://e.org/a.jsonld", "https://e.org/b.jsonld"), &BrokerRoot, WrapperUse::Response)
        .await
        .expect("hosted");
    let reordered = registry
        .create_implicitly(compound("https://e.org/b.jsonld", "https://e.org/a.jsonld"), &BrokerRoot, WrapperUse::Response)
        .await
        .expect("hosted");

    assert_ne!(first, reordered, "JSON-LD processes an @context array in order");
    pretty_assertions::assert_eq!(store.len(), 2);
}

/// Anything other than an `ImplicitlyCreated` entry with the same content
/// holding the derived identifier (here a Hosted entry) sends the document to a
/// fresh random identifier, and the entry already there is left untouched.
#[tokio::test]
async fn a_derived_identifier_held_by_another_kind_of_entry_falls_back_to_a_random_one() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    let document = compound("https://e.org/a.jsonld", "https://e.org/b.jsonld");
    let derived = registry
        .create_implicitly(document.clone(), &BrokerRoot, WrapperUse::Response) // clone: the same document is hosted again below.
        .await
        .expect("hosted");
    registry
        .remove(&ContextIdentifier::from(Identifier::from(derived.clone())), ContextRemoval::Delete) // clone: the derived identifier is compared below.
        .await
        .expect("deleted");
    let hosted = json!({ "@context": { "Spot": "https://e.org/Spot" } });
    store.hold(&derived, ContextKind::Hosted, hosted.clone()); // clone: the held document is compared below.

    let fallback = registry.create_implicitly(document, &BrokerRoot, WrapperUse::Response).await.expect("hosted");

    assert_ne!(fallback, derived);
    pretty_assertions::assert_eq!(uuid_version(&fallback), Some(Version::Random));
    pretty_assertions::assert_eq!(
        store.read(derived.as_ref(), |row| (row.kind.clone(), row.payload.clone())),
        Some((ContextKind::Hosted, hosted))
    ); // clone: the read hands back an owned kind and document.
}

/// An `ImplicitlyCreated` entry holding different content under the derived
/// identifier (a hash collision) is never served for this document.
#[tokio::test]
async fn a_derived_identifier_held_by_different_implicit_content_falls_back_to_a_random_one() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    let document = compound("https://e.org/a.jsonld", "https://e.org/b.jsonld");
    let derived = registry
        .create_implicitly(document.clone(), &BrokerRoot, WrapperUse::Response) // clone: the same document is hosted again below.
        .await
        .expect("hosted");
    registry
        .remove(&ContextIdentifier::from(Identifier::from(derived.clone())), ContextRemoval::Delete) // clone: the derived identifier is compared below.
        .await
        .expect("deleted");
    store.hold(
        &derived,
        ContextKind::ImplicitlyCreated,
        compound("https://e.org/x.jsonld", "https://e.org/y.jsonld"),
    );

    let fallback = registry.create_implicitly(document, &BrokerRoot, WrapperUse::Response).await.expect("hosted");

    assert_ne!(fallback, derived);
    pretty_assertions::assert_eq!(uuid_version(&fallback), Some(Version::Random));
}

/// Clause 5.13.5: deleting a shared entry breaks its URL until the document is
/// needed again, when it comes back under the same identifier.
#[tokio::test]
async fn a_deleted_implicit_entry_comes_back_under_the_same_identifier() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    let document = compound("https://e.org/a.jsonld", "https://e.org/b.jsonld");
    let first = registry
        .create_implicitly(document.clone(), &BrokerRoot, WrapperUse::Response) // clone: the same document is hosted again below.
        .await
        .expect("hosted");
    registry
        .remove(&ContextIdentifier::from(Identifier::from(first.clone())), ContextRemoval::Delete) // clone: the first identifier is compared below.
        .await
        .expect("deleted");

    let again = registry.create_implicitly(document, &BrokerRoot, WrapperUse::Response).await.expect("hosted");

    pretty_assertions::assert_eq!(again, first);
}

/// A number beyond the range of a double has no RFC 8785 canonical form, so
/// the document cannot be shared and takes a random identifier.
#[tokio::test]
async fn a_document_with_no_canonical_form_takes_a_random_identifier() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);
    let document: Value = serde_json::from_str(r#"{"@context": [{"@version": 1e400}, "https://e.org/a.jsonld"]}"#).expect("JSON");

    let identifier = registry.create_implicitly(document, &BrokerRoot, WrapperUse::Response).await.expect("hosted");

    pretty_assertions::assert_eq!(uuid_version(&identifier), Some(Version::Random));
}

/// Annex C.8: a compound response context is hosted as an `ImplicitlyCreated`
/// wrapper under the identifier its content derives, so a request repeated
/// twice names one stored document, written once.
#[tokio::test]
async fn a_compound_response_context_hosted_twice_is_one_implicit_entry() {
    let store = Arc::new(MemoryStore::default());
    let registry = service(&store);

    let first = registry
        .create_implicitly(compound("https://e.org/a.jsonld", "https://e.org/b.jsonld"), &BrokerRoot, WrapperUse::Response)
        .await
        .expect("hosted");
    let second = registry
        .create_implicitly(compound("https://e.org/a.jsonld", "https://e.org/b.jsonld"), &BrokerRoot, WrapperUse::Response)
        .await
        .expect("hosted");

    pretty_assertions::assert_eq!(first, second);
    pretty_assertions::assert_eq!(uuid_version(&first), Some(Version::Sha1));
    pretty_assertions::assert_eq!(store.len(), 1);
    pretty_assertions::assert_eq!(
        store.read(first.as_ref(), |row| (row.kind.clone(), row.payload.clone())), // clone: the read hands back an owned kind and document.
        Some((
            ContextKind::ImplicitlyCreated,
            json!({ "@context": ["https://e.org/a.jsonld", "https://e.org/b.jsonld"] })
        ))
    );
}
