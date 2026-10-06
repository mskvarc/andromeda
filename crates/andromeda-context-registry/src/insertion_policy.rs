//! Which identifier a new Context is stored under (clauses 5.13.1 and 5.13.2.4).
//!
//! Clause 5.13.2.4 makes every Add @context "a new entry", so a Hosted Context
//! takes a fresh random identifier each time. An `ImplicitlyCreated` Context is
//! one the broker stores on its own initiative, so equal documents share one
//! entry under an identifier derived from their content ([`ContentIdentity`],
//! computed once per document). Both the Context Registry service and the
//! compound-context host store Contexts, and both decide the identifier here.

use crate::{
    canonical_document::CanonicalContextDocument,
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    hosted_context_locator::HostedContextLocator,
    insertion::ContextInsertion,
    registry_invariant::RegistryInvariant,
    store::ContextRegistryStore,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;

/// Stores new Contexts under the identifier clause 5.13 leaves to the broker.
pub(crate) struct InsertionPolicy<'a> {
    /// Where the Contexts are stored.
    store: &'a dyn ContextRegistryStore,
    /// Names the URL each new entry is served under, recorded as its `url`.
    locator: &'a dyn HostedContextLocator,
}

impl<'a> InsertionPolicy<'a> {
    /// The policy over `store`, recording the serving URLs `locator` names.
    pub(crate) const fn over(store: &'a dyn ContextRegistryStore, locator: &'a dyn HostedContextLocator) -> InsertionPolicy<'a> {
        InsertionPolicy { store, locator }
    }

    /// Store `document` under a fresh random identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Persistence`] when the store cannot
    /// record the Context, and [`ContextRegistryError::Invariant`] when the
    /// minted identifier is not one or is already held.
    pub(crate) async fn fresh(&self, document: &JsonLiteral, kind: ContextCreationKind) -> Result<JsonLdContextId, ContextRegistryError> {
        let identifier = JsonLdContextId::mint().map_err(RegistryInvariant::MintedIdentifier)?;
        let serving_url = self.locator.serving_url(&identifier);
        match self.store.insert(&identifier, document, &serving_url, kind).await? {
            ContextInsertion::Inserted => Ok(identifier),
            ContextInsertion::Occupied => Err(RegistryInvariant::MintedIdentifierTaken { identifier }.into()),
        }
    }

    /// Store `document` as an `ImplicitlyCreated` Context under the identifier
    /// its content derives.
    ///
    /// The identifier is the version 5 UUID of the RFC 8785 canonical form of
    /// the document ([`JsonLdContextId::content_derived`]), so equal documents
    /// share one entry, and a document deleted through clause 5.13.5 comes back
    /// under the same identifier the next time it is needed. The derived
    /// identifier is reused only when the entry holding it is an
    /// `ImplicitlyCreated` one with the same canonical content; anything else
    /// holding it (a hash collision) gets the document a fresh random identifier
    /// instead, so a different document is never served under a shared one. An
    /// entry that vanished between the insert finding the identifier held and
    /// the check reading it was deleted concurrently, which breaks nothing about
    /// sharing, so the derived insert is tried once more before falling back. A
    /// document with no canonical form ([`ContentIdentity::Unshareable`]),
    /// which only a number beyond the range of an IEEE 754 double produces,
    /// cannot be shared and takes a fresh random identifier as well; this is the
    /// one place that fallback is decided.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Persistence`] when the store cannot
    /// record or read the Context, and [`ContextRegistryError::Invariant`] when
    /// a freshly minted identifier is not one or is already held.
    pub(crate) async fn content_derived(&self, document: &JsonLiteral, identity: &ContentIdentity) -> Result<JsonLdContextId, ContextRegistryError> {
        let ContentIdentity::Derived { canonical, identifier } = identity else {
            return self.fresh(document, ContextCreationKind::ImplicitlyCreated).await;
        };
        let shared = match self.insert_derived(identifier, document, canonical).await? {
            DerivedInsertion::Vanished => self.insert_derived(identifier, document, canonical).await?,
            settled @ (DerivedInsertion::Stored | DerivedInsertion::HeldByAnother) => settled,
        };
        match shared {
            DerivedInsertion::Stored => Ok(identifier.clone()), // clone: the identity keeps its identifier for the caller's memo
            DerivedInsertion::HeldByAnother | DerivedInsertion::Vanished => self.fresh(document, ContextCreationKind::ImplicitlyCreated).await,
        }
    }

    /// Insert `document` under its derived identifier, and when the identifier
    /// is already held, find out by what.
    async fn insert_derived(
        &self,
        derived: &JsonLdContextId,
        document: &JsonLiteral,
        canonical: &CanonicalContextDocument,
    ) -> Result<DerivedInsertion, ContextRegistryError> {
        let serving_url = self.locator.serving_url(derived);
        match self
            .store
            .insert(derived, document, &serving_url, ContextCreationKind::ImplicitlyCreated)
            .await?
        {
            ContextInsertion::Inserted => Ok(DerivedInsertion::Stored),
            ContextInsertion::Occupied => {
                let held = Identifier::from(derived.clone()); // clone: the caller returns the derived identifier when the held entry matches.
                self.held_entry(&held, canonical).await
            }
        }
    }

    /// What holds `identifier`, relative to a document whose content is
    /// `canonical`.
    async fn held_entry(&self, identifier: &Identifier, canonical: &CanonicalContextDocument) -> Result<DerivedInsertion, ContextRegistryError> {
        let Some(stored) = self.store.lookup(identifier).await? else {
            return Ok(DerivedInsertion::Vanished);
        };
        if stored.kind != ContextKind::ImplicitlyCreated {
            return Ok(DerivedInsertion::HeldByAnother);
        }
        Ok(match self.store.get(identifier, ContextDetailLevel::Summary).await? {
            Some(FetchedContext::Payload(held)) if CanonicalContextDocument::of(&held).is_ok_and(|held| held == *canonical) => DerivedInsertion::Stored,
            Some(FetchedContext::Payload(_) | FetchedContext::Detailed(_)) => DerivedInsertion::HeldByAnother,
            None => DerivedInsertion::Vanished,
        })
    }
}

/// What an `ImplicitlyCreated` document shares its entry by: the identifier its
/// content derives, computed once from its RFC 8785 canonical form.
#[derive(Debug)]
pub(crate) enum ContentIdentity {
    /// The document's canonical form and the version 5 identifier it derives
    /// ([`JsonLdContextId::content_derived`]).
    Derived {
        /// The canonical form an entry holding the identifier is compared by.
        canonical: CanonicalContextDocument,
        /// The identifier equal documents share.
        identifier: JsonLdContextId,
    },
    /// The document has no canonical form, so it cannot be shared.
    Unshareable,
}

impl ContentIdentity {
    /// The content identity of `document`.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Invariant`] when the derived identifier
    /// cannot be built, which the lexicon proves never happens.
    pub(crate) fn of(document: &JsonLiteral) -> Result<ContentIdentity, ContextRegistryError> {
        let Ok(canonical) = CanonicalContextDocument::of(document) else {
            return Ok(ContentIdentity::Unshareable);
        };
        let identifier = JsonLdContextId::content_derived(canonical.as_bytes()).map_err(RegistryInvariant::MintedIdentifier)?;
        Ok(ContentIdentity::Derived { canonical, identifier })
    }
}

/// Where one attempt to store a document under its derived identifier left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DerivedInsertion {
    /// The document is stored under the identifier, by this attempt or an
    /// equal `ImplicitlyCreated` one before it.
    Stored,
    /// Another document, or another kind of entry, holds the identifier.
    HeldByAnother,
    /// The identifier was held when inserting and nothing held it when checked.
    Vanished,
}

#[cfg(test)]
mod tests {
    use crate::{
        creation_kind::ContextCreationKind,
        detail_level::ContextDetailLevel,
        fetched_context::FetchedContext,
        hosted_context_locator::HostedContextLocator,
        insertion::ContextInsertion,
        insertion_policy::{ContentIdentity, InsertionPolicy},
        list_query::ContextListQuery,
        listed_contexts::ListedContexts,
        mutation::ContextMutation,
        store::{ContextRegistryFuture, ContextRegistryStore},
        stored_context::StoredContext,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_cache::DownloadedContext;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
    use serde_json::{Value, json};
    use std::sync::Mutex;
    use url::Url;
    use uuid::{Uuid, Version};

    fn literal(value: &Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible document")
    }

    /// A store whose inserts answer from a script, and which never holds a row
    /// when asked: the row the first insert collided with was deleted before
    /// the check could read it.
    struct VanishingStore {
        /// The answers the next inserts give, in order.
        inserts: Mutex<Vec<ContextInsertion>>,
        /// The identifiers the inserts were offered, in order.
        offered: Mutex<Vec<JsonLdContextId>>,
    }

    impl VanishingStore {
        fn scripted(inserts: Vec<ContextInsertion>) -> VanishingStore {
            VanishingStore {
                inserts: Mutex::new(inserts),
                offered: Mutex::new(Vec::new()),
            }
        }
    }

    impl ContextRegistryStore for VanishingStore {
        fn insert<'a>(
            &'a self,
            identifier: &'a JsonLdContextId,
            _payload: &'a JsonLiteral,
            _serving_url: &'a Url,
            _kind: ContextCreationKind,
        ) -> ContextRegistryFuture<'a, ContextInsertion> {
            Box::pin(async move {
                self.offered.lock().expect("the offer log").push(identifier.clone()); // clone: the log keeps what was offered
                let mut inserts = self.inserts.lock().expect("the script");
                Ok(if inserts.is_empty() { ContextInsertion::Inserted } else { inserts.remove(0) })
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
            let mut url = Url::parse("https://broker.example/").expect("a URL");
            url.set_path(identifier.as_ref());
            url
        }
    }

    /// The content identity of `document`.
    fn identity(document: &JsonLiteral) -> ContentIdentity {
        ContentIdentity::of(document).expect("an identity")
    }

    /// The version of the UUID an identifier carries as its local id.
    fn uuid_version(identifier: &JsonLdContextId) -> Option<Version> {
        let text = identifier.to_string();
        let local_id = text.strip_prefix("urn:ngsi-ld:JsonLdContext:").expect("the JsonLdContext prefix");
        Uuid::parse_str(local_id).expect("a hyphenated UUID").get_version()
    }

    fn compound() -> JsonLiteral {
        literal(&json!({ "@context": ["https://e.org/a.jsonld", "https://e.org/b.jsonld"] }))
    }

    /// A row that vanished between the insert finding the identifier held and
    /// the check reading it was deleted concurrently; the derived insert is
    /// tried again and lands, so the document keeps its shared identifier.
    #[tokio::test]
    async fn a_row_that_vanished_after_a_collision_is_inserted_again_under_the_derived_identifier() {
        let store = VanishingStore::scripted(vec![ContextInsertion::Occupied, ContextInsertion::Inserted]);

        let identifier = InsertionPolicy::over(&store, &FixedRoot)
            .content_derived(&compound(), &identity(&compound()))
            .await
            .expect("stored");

        pretty_assertions::assert_eq!(uuid_version(&identifier), Some(Version::Sha1));
        let offered = store.offered.lock().expect("the offer log");
        pretty_assertions::assert_eq!(offered.len(), 2, "one retry of the derived insert");
        pretty_assertions::assert_eq!(offered[0], offered[1]);
    }

    /// The retry happens once: a second collision whose row vanished again
    /// falls back to a fresh random identifier.
    #[tokio::test]
    async fn a_second_vanished_collision_falls_back_to_a_random_identifier() {
        let store = VanishingStore::scripted(vec![ContextInsertion::Occupied, ContextInsertion::Occupied, ContextInsertion::Inserted]);

        let identifier = InsertionPolicy::over(&store, &FixedRoot)
            .content_derived(&compound(), &identity(&compound()))
            .await
            .expect("stored");

        pretty_assertions::assert_eq!(uuid_version(&identifier), Some(Version::Random));
        pretty_assertions::assert_eq!(store.offered.lock().expect("the offer log").len(), 3);
    }

    /// The identity is computed once per document: equal content, whatever its
    /// member order, derives one identifier, and a document with no canonical
    /// form is unshareable.
    #[test]
    fn a_content_identity_is_derived_once_from_the_canonical_form() {
        let forward = JsonLiteral::admit(r#"{"@context": {"a": "https://e.org/a", "b": "https://e.org/b"}}"#).expect("JSON");
        let backward = JsonLiteral::admit(r#"{"@context":{"b":"https://e.org/b","a":"https://e.org/a"}}"#).expect("JSON");
        let (ContentIdentity::Derived { identifier: first, .. }, ContentIdentity::Derived { identifier: second, .. }) =
            (identity(&forward), identity(&backward))
        else {
            panic!("both documents have a canonical form");
        };
        pretty_assertions::assert_eq!(first, second);

        let unshareable = JsonLiteral::admit(r#"{"@context": {"@version": 1e400}}"#).expect("an admissible literal");
        assert!(matches!(identity(&unshareable), ContentIdentity::Unshareable));
    }
}
