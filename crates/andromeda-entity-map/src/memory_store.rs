//! Concurrent in-process implementation of the Entity Map storage port.

use crate::store::{EntityMapMutation, EntityMapStore, EntityMapStoreFuture};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;

/// Concurrent in-process Entity Map store with opportunistic expiry cleanup.
#[derive(Debug, Default)]
pub struct InMemoryEntityMapStore {
    /// Live and not-yet-cleaned expired maps keyed by identifier.
    maps: DashMap<Uri, EntityMap>,
}

impl InMemoryEntityMapStore {
    /// Create an empty in-process Entity Map store.
    pub fn new() -> InMemoryEntityMapStore {
        InMemoryEntityMapStore { maps: DashMap::new() }
    }
}

impl EntityMapStore for InMemoryEntityMapStore {
    fn insert<'a>(&'a self, map: &'a EntityMap) -> EntityMapStoreFuture<'a, ()> {
        Box::pin(async move {
            self.maps.retain(|_, stored| !stored.is_expired());
            self.maps.insert(map.id.clone(), map.clone()); // clone: the store must own both the key and value while the policy retains its response.
            Ok(())
        })
    }

    fn get<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, Option<EntityMap>> {
        Box::pin(async move {
            let stored = self.maps.get(identifier);
            match stored {
                Some(stored) if stored.is_expired() => {
                    drop(stored);
                    self.maps.remove_if(identifier, |_, map| map.is_expired());
                    Ok(None)
                }
                Some(stored) => Ok(Some(stored.clone())), // clone: the returned map must outlive the DashMap read guard.
                None => Ok(None),
            }
        })
    }

    fn update<'a>(&'a self, identifier: &'a Uri, expires_at: DateTime<Utc>) -> EntityMapStoreFuture<'a, EntityMapMutation> {
        Box::pin(async move {
            let stored = self.maps.get_mut(identifier);
            match stored {
                Some(mut stored) if !stored.is_expired() => {
                    stored.expires_at = expires_at;
                    Ok(EntityMapMutation::Applied)
                }
                Some(stored) => {
                    drop(stored);
                    self.maps.remove_if(identifier, |_, map| map.is_expired());
                    Ok(EntityMapMutation::Missing)
                }
                None => Ok(EntityMapMutation::Missing),
            }
        })
    }

    fn delete<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
        Box::pin(async move {
            let removed = self.maps.remove_if(identifier, |_, map| !map.is_expired());
            if removed.is_some() {
                Ok(EntityMapMutation::Applied)
            } else {
                self.maps.remove_if(identifier, |_, map| map.is_expired());
                Ok(EntityMapMutation::Missing)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::EntityMapError, memory_store::InMemoryEntityMapStore, store::EntityMapStore};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn expired_maps_are_removed_when_read() -> Result<(), EntityMapError> {
        let identifier = "urn:ngsi-ld:EntityMap:expired"
            .parse::<Uri>()
            .map_err(|error| EntityMapError::Invariant { detail: error.to_string() })?;
        let map = EntityMap::new(identifier.clone(), Utc::now() - Duration::seconds(1), Vec::new()); // clone: the assertion needs the identifier after map construction.
        let store = InMemoryEntityMapStore::new();
        store.insert(&map).await?;
        assert!(store.get(&identifier).await?.is_none());
        Ok(())
    }
}
