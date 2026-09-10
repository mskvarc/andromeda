//! Recording what a read did to the Entity Map it ran under (clauses 4.3.6.7
//! and 5.5.14).
//!
//! Every read that runs under a map ends the same way, whichever operation it
//! was: a reused map records the maps its sources answered with and sheds the
//! entities this page checked and found no longer matching; a minted one is
//! filled with the entities this broker holds and the ones each source
//! returned, then stored; and a read under no map records nothing. What differs
//! between the operations is only how the entities this broker holds are
//! listed, and whether the page applied the request's filters at all — the two
//! parameters of [`EntityMapService::record_read`].

use crate::{contribution::SourceContribution, error::EntityMapError, pruning::PageCheck, reuse::EntityMapUse, service::EntityMapService};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::future::Future;

/// Lists the entities this broker itself contributes to the map a read runs
/// under (clause 5.2.39: the `EntityMapSource` that is not a registration).
///
/// Listing them is not free — a query pays a pass over its own store for it —
/// so the recording asks only in the cases that record them, and the binding
/// that implements this decides how the listing is made and what its failure
/// looks like.
pub trait LocalMapEntities {
    /// How the binding reports a listing it could not make.
    type Error;

    /// The identifiers this broker contributes to the map, for the purpose
    /// `listing` names.
    ///
    /// # Errors
    ///
    /// Whatever the binding reports when the entities cannot be listed, which
    /// for an HTTP binding is the problem response the failure classifies to.
    fn identifiers(&self, listing: MapListing) -> impl Future<Output = Result<Vec<Uri>, Self::Error>> + Send;
}

/// Why the entities this broker holds are being listed.
///
/// The two purposes ask different questions of the same request, and answering
/// the second with the first is how a map loses entities nobody asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapListing {
    /// For a map being minted: the entities this request matched, which are the
    /// ones the map fixes for the requests that follow (clause 5.5.14).
    Minted,
    /// For the recheck a prepared page performs: the entities that still match
    /// the filters of the request the map was created for (clause 4.3.6.7).
    ///
    /// The entity identifiers a request specifies are not among those filters:
    /// clause 5.5.14 holds a follow-up to "the same parameters as in the
    /// original request that created the `EntityMap`, except for the
    /// specification of Entity identifiers", so a follow-up asking about one
    /// entity must not shed from the map the entities it did not ask about.
    Rechecked,
}

/// Whether preparing this read's page applied the request's filters to the
/// entities of the map it ran under (clause 4.3.6.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFiltering {
    /// A query applies its query, geoquery, scope query and attribute filters
    /// to an entity before it joins a result page, so the entities it checked
    /// and found no longer matching leave the map.
    Applied,
    /// A retrieval applies none of them — it reads the map only to learn which
    /// context sources hold the one entity it asked for — so it checks nothing
    /// and the map keeps every identifier it holds.
    NotApplied,
}

/// A failure while recording what a read did to the map it ran under.
///
/// The two halves fail in different vocabularies: listing the entities this
/// broker holds is the binding's own operation, while reading and writing the
/// map is this crate's, and neither can classify the other's failure.
#[derive(Debug)]
pub enum RecordingFailure<E> {
    /// The binding could not list the entities this broker contributes.
    LocalEntities(E),
    /// The map store could not be read or written.
    Store(EntityMapError),
}

impl EntityMapService {
    /// Record what a read did to the map it ran under, and answer with that
    /// map's identifier (Table 6.5.3.1-1 `entityMap`).
    ///
    /// Clause 5.5.14: a reused map records the maps its sources answered with
    /// and, when this page applied the request's filters, sheds the entities it
    /// found no longer matching (clause 4.3.6.7); a minted one names this
    /// broker and every registration that returned an entity, each entity
    /// attributed to the sources that hold it (clause 5.2.39). A read under no
    /// map records nothing and names none.
    ///
    /// # Errors
    ///
    /// [`RecordingFailure::LocalEntities`] when `local` cannot list what this
    /// broker holds, and [`RecordingFailure::Store`] when the map store cannot
    /// be read or written.
    pub async fn record_read<L>(
        &self,
        map_use: EntityMapUse,
        contributions: Vec<SourceContribution>,
        local: &L,
        filtering: PageFiltering,
    ) -> Result<Option<Uri>, RecordingFailure<L::Error>>
    where
        L: LocalMapEntities + Sync,
    {
        match map_use {
            EntityMapUse::Reuse(map) => {
                let links = contributions
                    .iter()
                    .filter_map(|contribution| contribution.linked_map.as_ref().map(|linked| (&contribution.registration, linked)));
                self.record_linked_maps(&map.id, links).await.map_err(RecordingFailure::Store)?;
                match filtering {
                    // Clause 5.5.9.3: the entities of a reused map are rechecked
                    // while the page is prepared, and the ones that no longer
                    // fit the query leave the map instead of being offered to
                    // the pages that follow.
                    PageFiltering::Applied => {
                        let mut matching = local.identifiers(MapListing::Rechecked).await.map_err(RecordingFailure::LocalEntities)?;
                        let mut consulted = Vec::with_capacity(contributions.len());
                        for contribution in contributions {
                            consulted.push(contribution.registration);
                            matching.extend(contribution.entity_ids);
                        }
                        self.prune_unmatched(&map, &PageCheck::new(matching, consulted))
                            .await
                            .map_err(RecordingFailure::Store)?;
                    }
                    PageFiltering::NotApplied => {}
                }
                Ok(Some(map.id))
            }
            EntityMapUse::Mint(mut map) => {
                map.add_local_entities(local.identifiers(MapListing::Minted).await.map_err(RecordingFailure::LocalEntities)?);
                for contribution in contributions {
                    map.add_contribution(&contribution.registration, contribution.entity_ids, contribution.linked_map);
                }
                self.store(&map).await.map_err(RecordingFailure::Store)?;
                Ok(Some(map.id))
            }
            EntityMapUse::None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        contribution::SourceContribution,
        error::EntityMapError,
        recording::{LocalMapEntities, MapListing, PageFiltering, RecordingFailure},
        reuse::EntityMapUse,
        service::EntityMapService,
        store::{EntityMapMutation, EntityMapStore, EntityMapStoreFuture},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::{
        entity_map::EntityMap,
        entity_map_fingerprint::{MapRequestKind, RequestFingerprint},
    };
    use chrono::{DateTime, TimeZone, Utc};
    use std::{
        future::{Future, ready},
        sync::{Arc, Mutex},
    };

    /// A store over a vector, deciding liveness by `expiresAt` like the durable
    /// one, so the fold is exercised against real reads and writes.
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

        fn update<'a>(&'a self, _identifier: &'a Uri, _expires_at: DateTime<Utc>) -> EntityMapStoreFuture<'a, EntityMapMutation> {
            Box::pin(async move { Ok(EntityMapMutation::Missing) })
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

        fn delete<'a>(&'a self, _identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
            Box::pin(async move { Ok(EntityMapMutation::Missing) })
        }

        fn delete_expired(&self, _cutoff: DateTime<Utc>) -> EntityMapStoreFuture<'_, u64> {
            Box::pin(async move { Ok(0) })
        }
    }

    /// The identifiers a test says this broker holds, or the failure listing
    /// them reports, recording which purpose the recording asked for.
    struct Local {
        /// What the listing answers with.
        answer: Result<Vec<Uri>, &'static str>,
        /// The purposes the recording asked for, in order.
        asked: Mutex<Vec<MapListing>>,
    }

    impl Local {
        fn answering(answer: Result<Vec<Uri>, &'static str>) -> Local {
            Local {
                answer,
                asked: Mutex::new(Vec::new()),
            }
        }

        fn asked_for(&self) -> Vec<MapListing> {
            self.asked.lock().map(|asked| asked.clone()).unwrap_or_default() // clone: the assertion owns what it reads
        }
    }

    impl LocalMapEntities for Local {
        type Error = &'static str;

        fn identifiers(&self, listing: MapListing) -> impl Future<Output = Result<Vec<Uri>, &'static str>> {
            if let Ok(mut asked) = self.asked.lock() {
                asked.push(listing);
            }
            // clone: the recording owns the identifiers it records
            ready(self.answer.clone())
        }
    }

    fn identifier(value: &str) -> Result<Uri, EntityMapError> {
        value.parse::<Uri>().map_err(|error| EntityMapError::Invariant { detail: error.to_string() })
    }

    fn service() -> EntityMapService {
        EntityMapService::new(Arc::new(VecStore::default()))
    }

    fn map_of(id: &str, entity_ids: impl IntoIterator<Item = Uri>) -> Result<EntityMap, EntityMapError> {
        let expires_at = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().ok_or_else(|| EntityMapError::Invariant {
            detail: "valid instant".into(),
        })?;
        let mut map = EntityMap::local(identifier(id)?, expires_at, entity_ids);
        map.request_fingerprint = Some(RequestFingerprint::new(MapRequestKind::QueryEntities, [0; 32]));
        Ok(map)
    }

    /// Clause 4.3.6.7: a page that applied the request's filters sheds the
    /// entities it checked and found no longer matching.
    #[tokio::test]
    async fn a_filtered_page_sheds_what_it_no_longer_matched() -> Result<(), EntityMapError> {
        let service = service();
        let map = map_of("urn:ngsi-ld:EntityMap:1", [identifier("urn:a")?, identifier("urn:b")?])?;
        service.store(&map).await?;

        let local = Local::answering(Ok(vec![identifier("urn:a")?]));
        let recorded = service
            .record_read(EntityMapUse::Reuse(Box::new(map)), Vec::new(), &local, PageFiltering::Applied)
            .await;
        assert!(matches!(recorded, Ok(Some(_))));

        let stored = service.retrieve(&identifier("urn:ngsi-ld:EntityMap:1")?).await?;
        pretty_assertions::assert_eq!(stored.entity_ids().collect::<Vec<_>>(), vec![&identifier("urn:a")?]);
        pretty_assertions::assert_eq!(local.asked_for(), vec![MapListing::Rechecked]);
        Ok(())
    }

    /// Clause 4.3.6.7: a retrieval applies none of the query's filters, so it
    /// checks nothing and the map keeps every identifier it holds.
    #[tokio::test]
    async fn an_unfiltered_page_sheds_nothing() -> Result<(), EntityMapError> {
        let service = service();
        let map = map_of("urn:ngsi-ld:EntityMap:2", [identifier("urn:a")?, identifier("urn:b")?])?;
        service.store(&map).await?;

        let local = Local::answering(Err("a retrieval never lists them"));
        let recorded = service
            .record_read(EntityMapUse::Reuse(Box::new(map)), Vec::new(), &local, PageFiltering::NotApplied)
            .await;
        assert!(matches!(recorded, Ok(Some(_))));

        let stored = service.retrieve(&identifier("urn:ngsi-ld:EntityMap:2")?).await?;
        pretty_assertions::assert_eq!(stored.entity_ids().count(), 2);
        assert!(local.asked_for().is_empty(), "a retrieval lists nothing to recheck");
        Ok(())
    }

    /// Clause 5.2.39: a minted map names this broker and every registration
    /// that returned an entity, each entity attributed to the source holding it.
    #[tokio::test]
    async fn a_minted_map_records_the_local_and_the_forwarded_entities() -> Result<(), EntityMapError> {
        let service = service();
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:1")?;
        let linked = identifier("urn:ngsi-ld:EntityMap:peer")?;
        let contributions = vec![SourceContribution {
            registration: registration.clone(), // clone: the assertion below reads the registration again
            entity_ids: vec![identifier("urn:remote")?],
            linked_map: Some(linked.clone()), // clone: as above
        }];

        let local = Local::answering(Ok(vec![identifier("urn:local")?]));
        let recorded = service
            .record_read(
                EntityMapUse::Mint(Box::new(map_of("urn:ngsi-ld:EntityMap:3", [])?)),
                contributions,
                &local,
                PageFiltering::Applied,
            )
            .await;
        assert!(matches!(recorded, Ok(Some(_))));

        let stored = service.retrieve(&identifier("urn:ngsi-ld:EntityMap:3")?).await?;
        pretty_assertions::assert_eq!(
            stored.entity_ids().collect::<Vec<_>>(),
            vec![&identifier("urn:local")?, &identifier("urn:remote")?]
        );
        pretty_assertions::assert_eq!(stored.linked_map_for(&registration), Some(&linked));
        pretty_assertions::assert_eq!(local.asked_for(), vec![MapListing::Minted]);
        Ok(())
    }

    /// A read under no map records nothing and names no map, which is what a
    /// plain query that neither referenced nor asked for one does.
    #[tokio::test]
    async fn a_read_under_no_map_records_nothing() {
        let local = Local::answering(Err("no map, nothing to list"));
        let recorded = service().record_read(EntityMapUse::None, Vec::new(), &local, PageFiltering::Applied).await;
        assert!(matches!(recorded, Ok(None)));
    }

    /// A failed listing is reported in the binding's own vocabulary rather than
    /// turned into a map-store failure.
    #[tokio::test]
    async fn a_failed_listing_is_reported_as_the_bindings_own_failure() -> Result<(), EntityMapError> {
        let service = service();
        let local = Local::answering(Err("the store could not be read"));
        let recorded = service
            .record_read(
                EntityMapUse::Mint(Box::new(map_of("urn:ngsi-ld:EntityMap:4", [])?)),
                Vec::new(),
                &local,
                PageFiltering::Applied,
            )
            .await;
        assert!(matches!(recorded, Err(RecordingFailure::LocalEntities("the store could not be read"))));
        Ok(())
    }
}
