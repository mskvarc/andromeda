//! Entity Map lifecycle against its store (clauses 5.5.14 and 5.14).

use crate::{
    error::EntityMapError,
    pruning::PageCheck,
    reuse::{EntityMapFlag, EntityMapUse},
    store::{EntityMapMutation, EntityMapStore},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    entity_map::EntityMap,
    entity_map_fingerprint::{RequestComparison, RequestFingerprint},
};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use std::sync::Arc;
use uuid::Uuid;

/// Manages NGSI-LD Entity Maps independently of their storage backend.
pub struct EntityMapService {
    /// The selected Entity Map storage implementation.
    store: Arc<dyn EntityMapStore>,
}

/// A map a request referenced through its `NGSILD-EntityMap` header, resolved
/// against the store (clause 5.5.14).
#[derive(Debug)]
pub enum ReferencedEntityMap {
    /// The map is live and fixes the entities the request considers.
    Live(Box<EntityMap>),
    /// The map has expired or never existed, so no inference can be made from
    /// it and the request creates a fresh one.
    Unavailable,
}

impl EntityMapService {
    /// Create an Entity Map service backed by the selected store.
    pub const fn new(store: Arc<dyn EntityMapStore>) -> EntityMapService {
        EntityMapService { store }
    }

    /// A fresh, unstored map over entities this broker holds for the request
    /// `fingerprint` describes, expiring after `lifetime` (one hour when the
    /// request suggests none).
    ///
    /// # Errors
    ///
    /// [`EntityMapError::Invariant`] when the generated identifier is not a
    /// valid URI.
    pub fn local_map(
        &self,
        entity_ids: impl IntoIterator<Item = Uri>,
        lifetime: Option<&Iso8601Duration>,
        fingerprint: RequestFingerprint,
    ) -> Result<EntityMap, EntityMapError> {
        let identifier = build_entity_map_identifier()?;
        let expires_at = Utc::now() + lifetime.copied().unwrap_or_else(default_lifetime);
        let mut map = EntityMap::local(identifier, expires_at, entity_ids);
        map.request_fingerprint = Some(fingerprint);
        Ok(map)
    }

    /// Store a composed map and make it accessible by its identifier.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::Persistence`] when the store cannot write the map.
    pub async fn store(&self, map: &EntityMap) -> Result<(), EntityMapError> {
        self.store.insert(map).await
    }

    /// Retrieve a live Entity Map by identifier.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::NotFound`] when no live map carries that identifier,
    /// and [`EntityMapError::Persistence`] when the store cannot be read.
    pub async fn retrieve(&self, identifier: &Uri) -> Result<EntityMap, EntityMapError> {
        self.store.get(identifier).await?.ok_or_else(|| EntityMapError::NotFound {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
        })
    }

    /// Resolve the map a request referenced: clause 5.5.14 says an expired or
    /// inaccessible map yields no inference and a new one is created, so a
    /// missing map is an outcome here rather than an error.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::Persistence`] when the store cannot be read.
    pub async fn referenced(&self, identifier: &Uri) -> Result<ReferencedEntityMap, EntityMapError> {
        Ok(match self.store.get(identifier).await? {
            Some(map) => ReferencedEntityMap::Live(Box::new(map)),
            None => ReferencedEntityMap::Unavailable,
        })
    }

    /// What a read does about Entity Maps (clause 5.5.14): resolve the map the
    /// request referenced, decide between reusing it, minting one (an empty
    /// map for the request `fingerprint` describes, living `lifetime`, to be
    /// filled and stored after the read) and none, and hold a reused map's
    /// follow-up to the parameters of the request that created it.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::RequestChanged`] when the same operation runs under
    /// the map with different filter parameters.
    pub async fn resolve_use(
        &self,
        referenced: Option<&Uri>,
        flag: EntityMapFlag,
        fingerprint: &RequestFingerprint,
        lifetime: Option<&Iso8601Duration>,
    ) -> Result<EntityMapUse, EntityMapError> {
        let referenced = match referenced {
            Some(identifier) => Some(self.referenced(identifier).await?),
            None => None,
        };
        let decided = EntityMapUse::decide(referenced, flag, || self.local_map(Vec::new(), lifetime, *fingerprint))?;
        if let Some(map) = decided.reused() {
            EntityMapService::require_same_request(map, fingerprint)?;
        }
        Ok(decided)
    }

    /// Resolve a map a request may only reuse and never create (Table
    /// 6.4.3.3-2), holding the follow-up to the creating request's parameters.
    ///
    /// Clause 5.5.14 lists the operations that may occur as part of a
    /// distributed transactional request, and Purge Entities is not among them,
    /// so a purge is never the request a map is created for; and Table
    /// 6.4.3.3-3 admits no 201 for it to report a creation through, where Table
    /// 6.4.3.2-3 gives Query Entities one. A referenced map that expired or was
    /// never stored therefore yields no inference and leaves the request scoped
    /// by nothing but its own query.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::RequestChanged`] when the request under the map
    /// changed a filter parameter of the request that created it, and
    /// [`EntityMapError::Persistence`] when the store cannot be read.
    pub async fn resolve_reuse(&self, referenced: &Uri, fingerprint: &RequestFingerprint) -> Result<ReferencedEntityMap, EntityMapError> {
        let resolved = self.referenced(referenced).await?;
        if let ReferencedEntityMap::Live(map) = &resolved {
            EntityMapService::require_same_request(map, fingerprint)?;
        }
        Ok(resolved)
    }

    /// Clause 5.5.14: a follow-up under a map uses the creating request's
    /// parameters. A map created before fingerprints were recorded, or by a
    /// different operation, cannot be compared and is accepted.
    fn require_same_request(map: &EntityMap, follow_up: &RequestFingerprint) -> Result<(), EntityMapError> {
        match map.request_fingerprint.as_ref().map(|created| created.compare(follow_up)) {
            Some(RequestComparison::Changed) => Err(EntityMapError::RequestChanged {
                identifier: map.id.clone(), // clone: the error must own the identifier after this call returns.
            }),
            Some(RequestComparison::Same | RequestComparison::DifferentOperation) | None => Ok(()),
        }
    }

    /// Update the expiry of a live Entity Map.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::NotFound`] when no live map carries that identifier,
    /// and [`EntityMapError::Persistence`] when the store cannot move the expiry.
    pub async fn update(&self, identifier: &Uri, expires_at: DateTime<Utc>) -> Result<(), EntityMapError> {
        match self.store.update(identifier, expires_at).await? {
            EntityMapMutation::Applied => Ok(()),
            EntityMapMutation::Missing => Err(EntityMapError::NotFound {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            }),
        }
    }

    /// Record on a live map the maps its sources answered a follow-up with
    /// (clause 5.5.14: "the `EntityMap`'s linked maps are updated to hold the
    /// location of every `EntityMap` used by the Context Source
    /// Registrations"). A map that expired meanwhile is left alone.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::Persistence`] when the store cannot record a link.
    pub async fn record_linked_maps<'a>(&self, identifier: &Uri, links: impl IntoIterator<Item = (&'a Uri, &'a Uri)>) -> Result<(), EntityMapError> {
        for (registration, linked_map) in links {
            self.store.link(identifier, registration, linked_map).await?;
        }
        Ok(())
    }

    /// Shed from a live map every entity a prepared result page checked and
    /// found no longer matching, returning the identifiers removed.
    ///
    /// Clause 4.3.6.7: the original request's filters are applied to an entity
    /// before it joins a page, and the identifiers of those that no longer match
    /// at the time of checking leave the map. Clause 5.5.9.3 says the same for
    /// the candidate entities a split-entity map holds, and clause 5.5.14 makes
    /// the broker that created the map responsible for the removal. A map that
    /// expired meanwhile is left alone.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::Persistence`] when the store cannot remove the
    /// identifiers.
    pub async fn prune_unmatched(&self, map: &EntityMap, check: &PageCheck) -> Result<Vec<Uri>, EntityMapError> {
        let shed = check.shed_from(map);
        if !shed.is_empty() {
            self.store.prune(&map.id, &shed).await?;
        }
        Ok(shed)
    }

    /// Delete a live Entity Map by identifier.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::NotFound`] when no live map carries that identifier,
    /// and [`EntityMapError::Persistence`] when the store cannot remove it.
    pub async fn delete(&self, identifier: &Uri) -> Result<(), EntityMapError> {
        match self.store.delete(identifier).await? {
            EntityMapMutation::Applied => Ok(()),
            EntityMapMutation::Missing => Err(EntityMapError::NotFound {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            }),
        }
    }
}

/// Build the default one-hour Entity Map lifetime without parsing text at runtime.
fn default_lifetime() -> Iso8601Duration {
    Iso8601Duration::new(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
}

/// Mint a new Entity Map identifier in the NGSI-LD URN namespace.
fn build_entity_map_identifier() -> Result<Uri, EntityMapError> {
    format!("urn:ngsi-ld:EntityMap:{}", Uuid::new_v4())
        .parse()
        .map_err(|error| EntityMapError::Invariant {
            detail: format!("generated Entity Map identifier is invalid: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityMapError,
        fingerprint::{FingerprintedRequest, fingerprint_of},
        pruning::PageCheck,
        query_scoping::scope_query_ids,
        reuse::{EntityMapFlag, EntityMapUse},
        service::{EntityMapService, ReferencedEntityMap},
        store::{EntityMapMutation, EntityMapStore, EntityMapStoreFuture},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::{
        entity_map::EntityMap,
        entity_map_fingerprint::{MapRequestKind, RequestFingerprint},
    };
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use chrono::{DateTime, Utc};
    use std::sync::{Arc, Mutex};

    /// A store over a vector, deciding liveness by `expiresAt` like the durable one.
    #[derive(Default)]
    struct VecStore(Mutex<Vec<EntityMap>>);

    impl EntityMapStore for VecStore {
        fn insert<'a>(&'a self, map: &'a EntityMap) -> EntityMapStoreFuture<'a, ()> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                maps.retain(|stored| stored.id != map.id);
                maps.push(map.clone()); // clone: the fake owns what it stores
                Ok(())
            })
        }

        fn get<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, Option<EntityMap>> {
            Box::pin(async move {
                let maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                Ok(maps.iter().find(|map| &map.id == identifier && !map.is_expired()).cloned()) // clone: the caller owns the returned map
            })
        }

        fn update<'a>(&'a self, identifier: &'a Uri, expires_at: DateTime<Utc>) -> EntityMapStoreFuture<'a, EntityMapMutation> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                match maps.iter_mut().find(|map| &map.id == identifier && !map.is_expired()) {
                    Some(map) => {
                        map.expires_at = expires_at;
                        Ok(EntityMapMutation::Applied)
                    }
                    None => Ok(EntityMapMutation::Missing),
                }
            })
        }

        fn link<'a>(&'a self, identifier: &'a Uri, registration: &'a Uri, linked_map: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                match maps.iter_mut().find(|map| &map.id == identifier && !map.is_expired()) {
                    Some(map) => {
                        map.link(registration.clone(), linked_map.clone()); // clone: the fake owns what it stores
                        Ok(EntityMapMutation::Applied)
                    }
                    None => Ok(EntityMapMutation::Missing),
                }
            })
        }

        fn prune<'a>(&'a self, identifier: &'a Uri, entity_ids: &'a [Uri]) -> EntityMapStoreFuture<'a, ()> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                if let Some(map) = maps.iter_mut().find(|map| &map.id == identifier && !map.is_expired()) {
                    map.entity_map.retain(|entity_id, _| !entity_ids.contains(entity_id));
                }
                Ok(())
            })
        }

        fn delete<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                let before = maps.len();
                maps.retain(|map| &map.id != identifier || map.is_expired());
                Ok(if maps.len() < before {
                    EntityMapMutation::Applied
                } else {
                    EntityMapMutation::Missing
                })
            })
        }

        fn delete_expired(&self, cutoff: DateTime<Utc>) -> EntityMapStoreFuture<'_, u64> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                let before = maps.len();
                maps.retain(|map| map.expires_at > cutoff);
                Ok((before - maps.len()) as u64)
            })
        }
    }

    fn identifier(value: &str) -> Result<Uri, EntityMapError> {
        value.parse::<Uri>().map_err(|error| EntityMapError::Invariant { detail: error.to_string() })
    }

    fn service() -> EntityMapService {
        EntityMapService::new(Arc::new(VecStore::default()))
    }

    fn query(raw: &str) -> Result<RequestFingerprint, EntityMapError> {
        let params: EntityQueryParameters = serde_urlencoded::from_str(raw).map_err(|error| EntityMapError::Invariant { detail: error.to_string() })?;
        fingerprint_of(FingerprintedRequest::QueryEntities(&params))
    }

    fn any_fingerprint() -> RequestFingerprint {
        RequestFingerprint::new(MapRequestKind::QueryEntities, [0; 32])
    }

    async fn stored_map(service: &EntityMapService, entity_ids: Vec<Uri>, fingerprint: RequestFingerprint) -> Result<EntityMap, EntityMapError> {
        let map = service.local_map(entity_ids, None, fingerprint)?;
        service.store(&map).await?;
        Ok(map)
    }

    #[tokio::test]
    async fn absent_lifetime_defaults_to_one_hour() -> Result<(), EntityMapError> {
        let before = Utc::now();
        let map = service().local_map(Vec::new(), None, any_fingerprint())?;
        let remaining = map.expires_at - before;
        assert!(remaining >= chrono::Duration::minutes(59));
        assert!(remaining <= chrono::Duration::minutes(61));
        pretty_assertions::assert_eq!(map.request_fingerprint, Some(any_fingerprint()));
        Ok(())
    }

    #[tokio::test]
    async fn a_referenced_map_that_is_missing_is_an_outcome_not_an_error() -> Result<(), EntityMapError> {
        let service = service();
        let map = stored_map(&service, vec![identifier("urn:a")?], any_fingerprint()).await?;
        assert!(matches!(service.referenced(&map.id).await?, ReferencedEntityMap::Live(_)));
        assert!(matches!(
            service.referenced(&identifier("urn:ngsi-ld:EntityMap:missing")?).await?,
            ReferencedEntityMap::Unavailable
        ));
        Ok(())
    }

    #[tokio::test]
    async fn deleting_an_absent_map_reports_not_found() -> Result<(), EntityMapError> {
        let result = service().delete(&identifier("urn:ngsi-ld:EntityMap:missing")?).await;
        assert!(matches!(result, Err(EntityMapError::NotFound { .. })));
        Ok(())
    }

    #[tokio::test]
    async fn a_stored_composed_map_is_retrieved_with_its_links() -> Result<(), EntityMapError> {
        let service = service();
        let mut map = service.local_map(vec![identifier("urn:a")?], None, any_fingerprint())?;
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:b")?;
        map.add_remote_entities(&registration, vec![identifier("urn:b")?]);
        service.store(&map).await?;
        let stored = service.retrieve(&map.id).await?;
        pretty_assertions::assert_eq!(stored.registrations().collect::<Vec<_>>(), vec![&registration]);
        Ok(())
    }

    /// Clause 5.5.14: the same query under its own map is accepted, a changed
    /// filter is refused, and a retrieval under a query's map is not compared.
    #[tokio::test]
    async fn a_reused_map_holds_the_follow_up_to_the_creating_parameters() -> Result<(), EntityMapError> {
        let service = service();
        let map = stored_map(&service, vec![identifier("urn:a")?], query("type=Vehicle")?).await?;

        let reused = service
            .resolve_use(Some(&map.id), EntityMapFlag::NotRequested, &query("type=Vehicle&limit=1&id=urn:a")?, None)
            .await?;
        assert!(matches!(&reused, EntityMapUse::Reuse(stored) if stored.id == map.id));

        let changed_filter = service
            .resolve_use(Some(&map.id), EntityMapFlag::Requested, &query("type=Vehicle&q=speed%3E99")?, None)
            .await;
        assert!(matches!(changed_filter, Err(EntityMapError::RequestChanged { identifier }) if identifier == map.id));

        let retrieval = RequestFingerprint::new(MapRequestKind::RetrieveEntity, [0; 32]);
        let other_operation = service.resolve_use(Some(&map.id), EntityMapFlag::NotRequested, &retrieval, None).await?;
        assert!(matches!(other_operation, EntityMapUse::Reuse(_)));
        Ok(())
    }

    /// Table 6.4.3.3-2: an operation that may only reuse a map takes the live
    /// one it was handed, is left with none when the reference cannot be
    /// resolved, and is still held to the creating request's filters.
    #[tokio::test]
    async fn a_reuse_only_reference_never_mints_a_replacement() -> Result<(), EntityMapError> {
        let service = service();
        let map = stored_map(&service, vec![identifier("urn:a")?], query("type=Vehicle")?).await?;

        let live = service.resolve_reuse(&map.id, &query("type=Vehicle&id=urn:a")?).await?;
        assert!(matches!(&live, ReferencedEntityMap::Live(stored) if stored.id == map.id));

        let missing = service
            .resolve_reuse(&identifier("urn:ngsi-ld:EntityMap:missing")?, &query("type=Vehicle")?)
            .await?;
        assert!(matches!(missing, ReferencedEntityMap::Unavailable));

        let changed = service.resolve_reuse(&map.id, &query("type=Building")?).await;
        assert!(matches!(changed, Err(EntityMapError::RequestChanged { identifier }) if identifier == map.id));
        Ok(())
    }

    #[tokio::test]
    async fn an_unavailable_reference_or_the_flag_alone_mints_a_map() -> Result<(), EntityMapError> {
        let service = service();
        let fingerprint = query("type=Vehicle")?;
        let bogus = identifier("urn:ngsi-ld:EntityMap:bogus")?;
        assert!(
            service
                .resolve_use(Some(&bogus), EntityMapFlag::NotRequested, &fingerprint, None)
                .await?
                .mints()
        );
        let minted = service.resolve_use(None, EntityMapFlag::Requested, &fingerprint, None).await?;
        let shell = minted.minting().ok_or_else(|| EntityMapError::Invariant {
            detail: "a map is minted".into(),
        })?;
        assert!(shell.entity_map.is_empty());
        pretty_assertions::assert_eq!(shell.request_fingerprint, Some(fingerprint));
        assert!(matches!(
            service.resolve_use(None, EntityMapFlag::NotRequested, &fingerprint, None).await?,
            EntityMapUse::None
        ));
        Ok(())
    }

    /// Clause 4.3.6.7: the identifiers a page preparation checked and found not
    /// matching leave the stored map, while the ones it still matched stay and a
    /// later page narrows to the survivors alone.
    #[tokio::test]
    async fn a_page_preparation_sheds_the_entities_it_found_no_longer_matching() -> Result<(), EntityMapError> {
        let service = service();
        let map = stored_map(
            &service,
            vec![identifier("urn:a")?, identifier("urn:b")?, identifier("urn:c")?],
            any_fingerprint(),
        )
        .await?;

        let check = PageCheck::new([identifier("urn:a")?, identifier("urn:c")?], Vec::new());
        let shed = service.prune_unmatched(&map, &check).await?;
        pretty_assertions::assert_eq!(shed, vec![identifier("urn:b")?]);

        let stored = service.retrieve(&map.id).await?;
        pretty_assertions::assert_eq!(stored.entity_ids().collect::<Vec<_>>(), vec![&identifier("urn:a")?, &identifier("urn:c")?]);
        pretty_assertions::assert_eq!(scope_query_ids(&stored, None), vec![identifier("urn:a")?, identifier("urn:c")?]);
        Ok(())
    }

    /// A preparation in which everything still matched writes nothing and leaves
    /// the map exactly as it was.
    #[tokio::test]
    async fn a_preparation_that_matched_everything_sheds_nothing() -> Result<(), EntityMapError> {
        let service = service();
        let map = stored_map(&service, vec![identifier("urn:a")?], any_fingerprint()).await?;
        let check = PageCheck::new([identifier("urn:a")?], Vec::new());
        pretty_assertions::assert_eq!(service.prune_unmatched(&map, &check).await?, Vec::<Uri>::new());
        pretty_assertions::assert_eq!(service.retrieve(&map.id).await?.entity_ids().count(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn linked_maps_are_recorded_on_the_stored_map() -> Result<(), EntityMapError> {
        let service = service();
        let map = stored_map(&service, vec![identifier("urn:a")?], any_fingerprint()).await?;
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:b")?;
        let linked = identifier("urn:ngsi-ld:EntityMap:peer")?;
        service.record_linked_maps(&map.id, [(&registration, &linked)]).await?;
        let stored = service.retrieve(&map.id).await?;
        pretty_assertions::assert_eq!(stored.linked_map_for(&registration), Some(&linked));
        Ok(())
    }
}
