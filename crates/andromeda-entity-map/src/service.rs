//! Entity Map lifecycle and query-intersection semantics.

use crate::{
    error::EntityMapError,
    store::{EntityMapMutation, EntityMapStore},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

/// Manages NGSI-LD Entity Maps independently of their storage backend.
pub struct EntityMapService {
    /// The selected Entity Map storage implementation.
    store: Arc<dyn EntityMapStore>,
}

impl EntityMapService {
    /// Create an Entity Map service backed by the selected store.
    pub const fn new(store: Arc<dyn EntityMapStore>) -> EntityMapService {
        EntityMapService { store }
    }

    /// Create an Entity Map from entity identifiers and an optional ISO 8601 lifetime.
    pub async fn create(&self, entity_ids: Vec<Uri>, lifetime: Option<&Iso8601Duration>) -> Result<EntityMap, EntityMapError> {
        let identifier = build_entity_map_identifier()?;
        let expires_at = Utc::now() + lifetime.copied().unwrap_or_else(default_lifetime);
        let map = EntityMap::new(identifier, expires_at, entity_ids);
        self.store.insert(&map).await?;
        Ok(map)
    }

    /// Retrieve a live Entity Map by identifier.
    pub async fn retrieve(&self, identifier: &Uri) -> Result<EntityMap, EntityMapError> {
        self.store.get(identifier).await?.ok_or_else(|| EntityMapError::NotFound {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
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

    /// Intersect query identifiers with a map while preserving query order.
    pub async fn scope_query_ids(&self, map_identifier: &Uri, existing_ids: Option<&[Uri]>) -> Result<Vec<Uri>, EntityMapError> {
        let map = self.retrieve(map_identifier).await?;
        match existing_ids {
            Some(existing) => {
                let map_ids: HashSet<&Uri> = map.entity_ids.iter().collect();
                Ok(existing
                    .iter()
                    .filter(|identifier| map_ids.contains(identifier))
                    .cloned() // clone: the result owns identifiers borrowed from the incoming query.
                    .collect())
            }
            None => Ok(map.entity_ids),
        }
    }

    /// Require an entity identifier to be a member of a live Entity Map.
    pub async fn validate_entity_in_map(&self, map_identifier: &Uri, entity_identifier: &Uri) -> Result<(), EntityMapError> {
        let map = self.retrieve(map_identifier).await?;
        if map.entity_ids.iter().any(|identifier| identifier == entity_identifier) {
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
    use crate::{error::EntityMapError, memory_store::InMemoryEntityMapStore, service::EntityMapService};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::sync::Arc;

    fn identifier(value: &str) -> Result<Uri, EntityMapError> {
        value.parse::<Uri>().map_err(|error| EntityMapError::Invariant { detail: error.to_string() })
    }

    #[tokio::test]
    async fn absent_lifetime_defaults_to_one_hour() -> Result<(), EntityMapError> {
        let before = chrono::Utc::now();
        let service = EntityMapService::new(Arc::new(InMemoryEntityMapStore::new()));
        let map = service.create(Vec::new(), None).await?;
        let remaining = map.expires_at - before;
        assert!(remaining >= chrono::Duration::minutes(59));
        assert!(remaining <= chrono::Duration::minutes(61));
        Ok(())
    }

    #[tokio::test]
    async fn intersection_preserves_incoming_query_order() -> Result<(), EntityMapError> {
        let service = EntityMapService::new(Arc::new(InMemoryEntityMapStore::new()));
        let map = service.create(vec![identifier("urn:a")?, identifier("urn:b")?], None).await?;
        let incoming = [identifier("urn:b")?, identifier("urn:a")?, identifier("urn:c")?];
        let scoped = service.scope_query_ids(&map.id, Some(&incoming)).await?;
        pretty_assertions::assert_eq!(scoped, vec![identifier("urn:b")?, identifier("urn:a")?]);
        Ok(())
    }

    #[tokio::test]
    async fn deleting_an_absent_map_reports_not_found() -> Result<(), EntityMapError> {
        let service = EntityMapService::new(Arc::new(InMemoryEntityMapStore::new()));
        let result = service.delete(&identifier("urn:ngsi-ld:EntityMap:missing")?).await;
        assert!(matches!(result, Err(EntityMapError::NotFound { .. })));
        Ok(())
    }
}
