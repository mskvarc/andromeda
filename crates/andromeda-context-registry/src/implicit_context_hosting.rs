//! Storing the `@context`s the broker hosts on its own initiative.
//!
//! An `application/json` response names its `@context` in exactly one `Link`
//! header (clause 6.3.6; JSON-LD 1.1 section 6.1), and a subscription's
//! `jsonldContext` is one URL (clause 5.8.1.4), so a context that is several
//! contexts, or one written inline, is named through a wrapper the broker hosts
//! (Annex C.8): an `ImplicitlyCreated` `@context`, "created by the broker as a
//! result of a user request" (clause 5.13.1). Equal documents share one entry
//! under the identifier their content derives.
//!
//! The identifier is computed in process (RFC 8785, then a version 5 UUID), and
//! the identifiers known to be stored are remembered for the span the broker
//! trusts its own copy of a hosted document, so a request repeated a thousand
//! times writes once. A wrapper deleted through clause 5.13.5 is forgotten at
//! once in this process, and in any other at the latest one span later, after
//! which it is stored again under the same identifier.

use crate::{
    error::ContextRegistryError,
    hosted_context_locator::HostedContextLocator,
    insertion_policy::{ContentIdentity, InsertionPolicy},
    registry_invariant::RegistryInvariant,
    store::ContextRegistryStore,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use moka::sync::Cache;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

/// How many stored identifiers one tenant remembers at most.
const REMEMBERED_IDENTIFIERS: u64 = 10_000;

/// Stores `ImplicitlyCreated` `@context`s, writing each distinct one once per
/// remembered span.
#[derive(derive_more::Debug)]
pub struct ImplicitContextHosting {
    /// Where the tenant's `@context`s are stored.
    #[debug(skip)]
    store: Arc<dyn ContextRegistryStore>,
    /// The identifier each content-derived identifier is known to be stored
    /// under: itself, or the random one a hash collision fell back to.
    #[debug(skip)]
    stored: Cache<JsonLdContextId, JsonLdContextId>,
}

impl ImplicitContextHosting {
    /// Hosting over `store`, remembering a stored identifier for `remembered`.
    #[must_use]
    pub fn new(store: Arc<dyn ContextRegistryStore>, remembered: Duration) -> ImplicitContextHosting {
        ImplicitContextHosting {
            store,
            stored: Cache::builder()
                .max_capacity(REMEMBERED_IDENTIFIERS)
                .time_to_live(remembered)
                .support_invalidation_closures()
                .build(),
        }
    }

    /// Store `document` as an `ImplicitlyCreated` `@context`, served under the
    /// URL `locator` names, and answer the identifier it is stored under.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Persistence`] when the store cannot
    /// record or read the Context, and [`ContextRegistryError::Invariant`] when
    /// an identifier cannot be built or a freshly minted one is already held.
    pub async fn host(&self, document: &Value, locator: &dyn HostedContextLocator) -> Result<JsonLdContextId, ContextRegistryError> {
        let policy = InsertionPolicy::over(self.store.as_ref(), locator);
        let identity = ContentIdentity::of(document)?;
        // A document with no canonical form cannot be shared, so it is stored
        // under a fresh identifier every time and never remembered.
        let ContentIdentity::Derived { identifier: ref derived, .. } = identity else {
            return policy.content_derived(document, &identity).await;
        };
        if let Some(stored) = self.stored.get(derived) {
            return Ok(stored);
        }
        let stored = policy.content_derived(document, &identity).await?;
        self.stored.insert(derived.clone(), stored.clone()); // clone: the memo keys on the derived identifier and keeps the one the caller is answered with
        Ok(stored)
    }

    /// Forget that `identifier` is stored, because it was just deleted.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Invariant`] when the memo cannot take
    /// the invalidation.
    pub fn forget(&self, identifier: &Identifier) -> Result<(), ContextRegistryError> {
        let deleted = identifier.clone(); // clone: the predicate outlives this call inside the memo
        self.stored
            .invalidate_entries_if(move |_, stored| Identifier::from(stored.clone()) == deleted) // clone: the comparison reads a remembered identifier the memo keeps
            .map(|_| ())
            .map_err(|error| RegistryInvariant::MemoInvalidation(error).into())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        creation_kind::ContextCreationKind,
        detail_level::ContextDetailLevel,
        fetched_context::FetchedContext,
        hosted_context_locator::HostedContextLocator,
        implicit_context_hosting::ImplicitContextHosting,
        insertion::ContextInsertion,
        list_query::ContextListQuery,
        listed_contexts::ListedContexts,
        mutation::ContextMutation,
        store::{ContextRegistryFuture, ContextRegistryStore},
        stored_context::StoredContext,
    };
    use andromeda_jsonld::context_cache::DownloadedContext;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
    use serde_json::{Value, json};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::time::sleep;
    use url::Url;

    /// A store that accepts every insert and records the serving URL of each.
    #[derive(Default)]
    struct CountingStore {
        inserts: Mutex<Vec<Url>>,
    }

    impl CountingStore {
        fn inserts(&self) -> usize {
            self.inserts.lock().expect("the insert log").len()
        }
    }

    impl ContextRegistryStore for CountingStore {
        fn insert<'a>(
            &'a self,
            _identifier: &'a JsonLdContextId,
            _payload: &'a Value,
            serving_url: &'a Url,
            _kind: ContextCreationKind,
        ) -> ContextRegistryFuture<'a, ContextInsertion> {
            Box::pin(async move {
                self.inserts.lock().expect("the insert log").push(serving_url.clone()); // clone: the log keeps what was stored
                Ok(ContextInsertion::Inserted)
            })
        }

        fn list<'a>(&'a self, _query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ListedContexts> {
            Box::pin(async move { Ok(ListedContexts::Summary(Vec::new())) })
        }

        fn get<'a>(&'a self, _identifier: &'a Identifier, _detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
            Box::pin(async move { Ok(None) })
        }

        fn record_access<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
            Box::pin(async move { Ok(ContextMutation::Missing) })
        }

        fn lookup<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
            Box::pin(async move { Ok(None) })
        }

        fn delete<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
            Box::pin(async move { Ok(ContextMutation::Missing) })
        }

        fn replace_download<'a>(&'a self, _identifier: &'a Identifier, _download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
            Box::pin(async move { Ok(ContextMutation::Missing) })
        }
    }

    /// Serves every entry under `https://broker.example/<identifier>`.
    struct FixedRoot;

    impl HostedContextLocator for FixedRoot {
        fn serving_url(&self, identifier: &JsonLdContextId) -> Url {
            Url::parse(&format!("https://broker.example/{identifier}")).expect("a URL")
        }
    }

    fn wrapper() -> Value {
        json!({ "@context": ["https://example.com/a.jsonld", "https://example.com/b.jsonld"] })
    }

    #[tokio::test]
    async fn hosting_the_same_document_twice_stores_it_once() {
        let store = Arc::new(CountingStore::default());
        let hosting = ImplicitContextHosting::new(Arc::clone(&store) as Arc<dyn ContextRegistryStore>, Duration::from_secs(60));

        let first = hosting.host(&wrapper(), &FixedRoot).await.expect("stored");
        let second = hosting.host(&wrapper(), &FixedRoot).await.expect("remembered");

        pretty_assertions::assert_eq!(first, second);
        pretty_assertions::assert_eq!(store.inserts(), 1);
    }

    #[tokio::test]
    async fn the_entry_records_the_url_the_locator_names() {
        let store = Arc::new(CountingStore::default());
        let hosting = ImplicitContextHosting::new(Arc::clone(&store) as Arc<dyn ContextRegistryStore>, Duration::from_secs(60));

        let identifier = hosting.host(&wrapper(), &FixedRoot).await.expect("stored");

        pretty_assertions::assert_eq!(
            *store.inserts.lock().expect("the insert log"),
            vec![Url::parse(&format!("https://broker.example/{identifier}")).expect("a URL")]
        );
    }

    /// Once the remembered span is over the document is stored again, so a
    /// wrapper deleted elsewhere comes back under the same identifier.
    #[tokio::test]
    async fn a_document_is_stored_again_once_the_remembered_span_is_over() {
        let store = Arc::new(CountingStore::default());
        let hosting = ImplicitContextHosting::new(Arc::clone(&store) as Arc<dyn ContextRegistryStore>, Duration::from_millis(20));

        let first = hosting.host(&wrapper(), &FixedRoot).await.expect("stored");
        sleep(Duration::from_millis(60)).await;
        let second = hosting.host(&wrapper(), &FixedRoot).await.expect("stored again");

        pretty_assertions::assert_eq!(first, second);
        pretty_assertions::assert_eq!(store.inserts(), 2);
    }

    #[tokio::test]
    async fn a_forgotten_identifier_is_stored_again_at_once() {
        let store = Arc::new(CountingStore::default());
        let hosting = ImplicitContextHosting::new(Arc::clone(&store) as Arc<dyn ContextRegistryStore>, Duration::from_secs(60));

        let identifier = hosting.host(&wrapper(), &FixedRoot).await.expect("stored");
        hosting.forget(&Identifier::from(identifier)).expect("forgotten");
        hosting.host(&wrapper(), &FixedRoot).await.expect("stored again");

        pretty_assertions::assert_eq!(store.inserts(), 2);
    }
}
