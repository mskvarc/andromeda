//! Entity Map lifecycle and query-intersection semantics (clauses 5.5.14 and 5.14).

use crate::{
    error::EntityMapError,
    store::{EntityMapMutation, EntityMapStore},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
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

    /// A fresh, unstored map over entities this broker holds, expiring after
    /// `lifetime` (one hour when the request suggests none).
    pub fn local_map(&self, entity_ids: impl IntoIterator<Item = Uri>, lifetime: Option<&Iso8601Duration>) -> Result<EntityMap, EntityMapError> {
        let identifier = build_entity_map_identifier()?;
        let expires_at = Utc::now() + lifetime.copied().unwrap_or_else(default_lifetime);
        Ok(EntityMap::local(identifier, expires_at, entity_ids))
    }

    /// Store a composed map and make it accessible by its identifier.
    pub async fn store(&self, map: &EntityMap) -> Result<(), EntityMapError> {
        self.store.insert(map).await
    }

    /// Create and store a map over entities this broker holds.
    pub async fn create_local(&self, entity_ids: impl IntoIterator<Item = Uri>, lifetime: Option<&Iso8601Duration>) -> Result<EntityMap, EntityMapError> {
        let map = self.local_map(entity_ids, lifetime)?;
        self.store.insert(&map).await?;
        Ok(map)
    }

    /// Retrieve a live Entity Map by identifier.
    pub async fn retrieve(&self, identifier: &Uri) -> Result<EntityMap, EntityMapError> {
        self.store.get(identifier).await?.ok_or_else(|| EntityMapError::NotFound {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
        })
    }

    /// Resolve the map a request referenced: clause 5.5.14 says an expired or
    /// inaccessible map yields no inference and a new one is created, so a
    /// missing map is an outcome here rather than an error.
    pub async fn referenced(&self, identifier: &Uri) -> Result<ReferencedEntityMap, EntityMapError> {
        Ok(match self.store.get(identifier).await? {
            Some(map) => ReferencedEntityMap::Live(Box::new(map)),
            None => ReferencedEntityMap::Unavailable,
        })
    }

    /// Update the expiry of a live Entity Map.
    pub async fn update(&self, identifier: &Uri, expires_at: DateTime<Utc>) -> Result<(), EntityMapError> {
        match self.store.update(identifier, expires_at).await? {
            EntityMapMutation::Applied => Ok(()),
            EntityMapMutation::Missing => Err(EntityMapError::NotFound {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            }),
        }
    }

    /// Intersect query identifiers with a live map while preserving query order.
    pub fn scope_query_ids(map: &EntityMap, existing_ids: Option<&[Uri]>) -> Vec<Uri> {
        match existing_ids {
            Some(existing) => existing
                .iter()
                .filter(|identifier| map.contains(identifier))
                .cloned() // clone: the result owns identifiers borrowed from the incoming query.
                .collect(),
            None => map.entity_ids().cloned().collect(), // clone: the result owns identifiers borrowed from the map.
        }
    }

    /// Require an entity identifier to be a member of a live Entity Map.
    pub async fn validate_entity_in_map(&self, map_identifier: &Uri, entity_identifier: &Uri) -> Result<(), EntityMapError> {
        let map = self.retrieve(map_identifier).await?;
        if map.contains(entity_identifier) {
            Ok(())
        } else {
            Err(EntityMapError::EntityNotInMap {
                map_identifier: map_identifier.clone(),       // clone: the error owns both identifiers beyond the borrowed call.
                entity_identifier: entity_identifier.clone(), // clone: the error owns both identifiers beyond the borrowed call.
            })
        }
    }

    /// Delete a live Entity Map by identifier.
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
        service::{EntityMapService, ReferencedEntityMap},
        store::{EntityMapMutation, EntityMapStore, EntityMapStoreFuture},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
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

        fn delete<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
            Box::pin(async move {
                let mut maps = self.0.lock().map_err(|_| EntityMapError::Invariant { detail: "poisoned".into() })?;
                let before = maps.len();
                maps.retain(|map| !(&map.id == identifier && !map.is_expired()));
                Ok(if maps.len() < before {
                    EntityMapMutation::Applied
                } else {
                    EntityMapMutation::Missing
                })
            })
        }

        fn delete_expired<'a>(&'a self, cutoff: DateTime<Utc>) -> EntityMapStoreFuture<'a, u64> {
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

    #[tokio::test]
    async fn absent_lifetime_defaults_to_one_hour() -> Result<(), EntityMapError> {
        let before = Utc::now();
        let map = service().create_local(Vec::new(), None).await?;
        let remaining = map.expires_at - before;
        assert!(remaining >= chrono::Duration::minutes(59));
        assert!(remaining <= chrono::Duration::minutes(61));
        Ok(())
    }

    #[tokio::test]
    async fn intersection_preserves_incoming_query_order() -> Result<(), EntityMapError> {
        let service = service();
        let map = service.create_local(vec![identifier("urn:a")?, identifier("urn:b")?], None).await?;
        let incoming = [identifier("urn:b")?, identifier("urn:a")?, identifier("urn:c")?];
        let scoped = EntityMapService::scope_query_ids(&map, Some(&incoming));
        pretty_assertions::assert_eq!(scoped, vec![identifier("urn:b")?, identifier("urn:a")?]);
        pretty_assertions::assert_eq!(EntityMapService::scope_query_ids(&map, None), vec![identifier("urn:a")?, identifier("urn:b")?]);
        Ok(())
    }

    #[tokio::test]
    async fn a_referenced_map_that_is_missing_is_an_outcome_not_an_error() -> Result<(), EntityMapError> {
        let service = service();
        let map = service.create_local(vec![identifier("urn:a")?], None).await?;
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
        let mut map = service.local_map(vec![identifier("urn:a")?], None)?;
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:b")?;
        map.add_remote_entities(&registration, vec![identifier("urn:b")?]);
        service.store(&map).await?;
        let stored = service.retrieve(&map.id).await?;
        pretty_assertions::assert_eq!(stored.registrations().collect::<Vec<_>>(), vec![&registration]);
        Ok(())
    }
}
