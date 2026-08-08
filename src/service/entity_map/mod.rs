/// The entity map store port.
pub mod store;

use crate::{core::error::AndromedaError, service::entity_map::store::EntityMapStore};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::EntityMapId, uri::Uri};
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use std::{collections::HashSet, sync::Arc};

/// Service for managing NGSI-LD entity maps.
pub struct EntityMapService {
    store: Arc<dyn EntityMapStore>,
}

impl EntityMapService {
    /// Create a new entity map service backed by the given store.
    pub const fn new(store: Arc<dyn EntityMapStore>) -> EntityMapService {
        EntityMapService { store }
    }

    /// Create a new entity map from the given entity IDs with an optional ISO8601 lifetime.
    pub async fn create(&self, entity_ids: Vec<Uri>, lifetime: Option<&Iso8601Duration>) -> Result<EntityMap, AndromedaError> {
        let id = build_entity_map_id()?;
        let expires_at = compute_entity_map_expires_at(Utc::now(), lifetime)?;
        let map = EntityMap::new(id, expires_at, entity_ids);
        self.store.insert(map.clone()).await; // clone: store takes ownership, caller needs return value
        Ok(map)
    }

    /// Retrieve an entity map by its identifier.
    pub async fn retrieve(&self, id: &EntityMapId) -> Result<EntityMap, AndromedaError> {
        self.store.get(id).await.ok_or_else(|| AndromedaError::entity_map_not_found(id.to_string()))
    }

    /// Update the expiration time of an existing entity map.
    pub async fn update(&self, id: &EntityMapId, expires_at: DateTime<Utc>) -> Result<(), AndromedaError> {
        if self.store.update(id, expires_at).await {
            Ok(())
        } else {
            Err(AndromedaError::entity_map_not_found(id.to_string()))
        }
    }

    /// Scope query IDs by intersecting with an EntityMap's entity set.
    pub async fn scope_query_ids(&self, map_id: &EntityMapId, existing_ids: Option<&[Uri]>) -> Result<Vec<Uri>, AndromedaError> {
        let map = self.retrieve(map_id).await?;
        if let Some(existing) = existing_ids {
            let query_ids: HashSet<Uri> = existing.iter().cloned().collect(); // clone: owned set elements for intersection
            let map_ids: HashSet<Uri> = map.entity_ids.into_iter().collect();
            let intersection: Vec<Uri> = query_ids.intersection(&map_ids).cloned().collect(); // clone: owned values from set intersection
            Ok(intersection)
        } else {
            Ok(map.entity_ids)
        }
    }

    /// Validate that a specific entity ID exists in the given EntityMap.
    pub async fn validate_entity_in_map(&self, map_id: &EntityMapId, entity_id: &urn_rs::Urn) -> Result<(), AndromedaError> {
        let map = self.retrieve(map_id).await?;
        if map.entity_ids.iter().any(|id| matches!(id, Uri::Urn(candidate) if candidate == entity_id)) {
            Ok(())
        } else {
            Err(AndromedaError::entity_map_not_found(entity_id.to_string()))
        }
    }

    /// Delete an entity map by its identifier.
    pub async fn delete(&self, id: &EntityMapId) -> Result<(), AndromedaError> {
        if self.store.delete(id).await {
            Ok(())
        } else {
            Err(AndromedaError::entity_map_not_found(id.to_string()))
        }
    }
}

/// Parse an ISO8601 duration string into `iso8601_duration::Duration`.
///
/// Returns a default of `PT1H` when `input` is `None` or empty.
pub fn parse_iso8601_duration(input: Option<&str>) -> Result<Iso8601Duration, AndromedaError> {
    match input {
        Some(raw) if !raw.trim().is_empty() => raw
            .trim()
            .parse::<Iso8601Duration>()
            .map_err(|e| AndromedaError::entity_map_validation(format!("Invalid ISO8601 duration '{}': {:?}", raw, e))),
        _ => "PT1H"
            .parse::<Iso8601Duration>()
            .map_err(|e| AndromedaError::internal(format!("Invalid default entityMapLifetime: {:?}", e))),
    }
}

fn build_entity_map_id() -> Result<EntityMapId, AndromedaError> {
    let value = format!("urn:ngsi-ld:EntityMap:{}", uuid::Uuid::new_v4());
    let id = value.parse::<EntityMapId>()?;
    Ok(id)
}

pub fn parse_entity_map_urn(raw: &str) -> Result<EntityMapId, AndromedaError> {
    let id = raw.parse::<EntityMapId>()?;
    Ok(id)
}

fn compute_entity_map_expires_at(created_at: DateTime<Utc>, lifetime: Option<&Iso8601Duration>) -> Result<DateTime<Utc>, AndromedaError> {
    let duration = match lifetime {
        Some(value) => *value,
        None => parse_iso8601_duration(None)?,
    };
    Ok(created_at + duration)
}

#[cfg(test)]
mod tests {
    use crate::service::entity_map::{parse_entity_map_urn, parse_iso8601_duration};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_postgresql_context_store::entity_map::InMemoryEntityMapStore;
    use std::{str::FromStr, sync::Arc};

    fn make_service() -> super::EntityMapService {
        super::EntityMapService::new(Arc::new(InMemoryEntityMapStore::new()))
    }

    #[tokio::test]
    async fn scope_query_ids_no_existing_returns_all_map_ids() {
        let svc = make_service();
        let map = svc
            .create(vec![Uri::from_str("urn:a").expect("uri"), Uri::from_str("urn:b").expect("uri")], None)
            .await
            .expect("create map");
        let result = svc.scope_query_ids(&map.id, None).await.expect("scope ids");
        assert!(result.iter().any(|id| id.to_string() == "urn:a"));
        assert!(result.iter().any(|id| id.to_string() == "urn:b"));
    }

    #[tokio::test]
    async fn scope_query_ids_with_overlap_returns_intersection() {
        let svc = make_service();
        let map = svc
            .create(
                vec![
                    Uri::from_str("urn:a").expect("uri"),
                    Uri::from_str("urn:b").expect("uri"),
                    Uri::from_str("urn:c").expect("uri"),
                ],
                None,
            )
            .await
            .expect("create map");
        let existing = vec![Uri::from_str("urn:b").expect("uri"), Uri::from_str("urn:d").expect("uri")];
        let result = svc.scope_query_ids(&map.id, Some(&existing)).await.expect("scope ids");
        assert!(result.iter().any(|id| id.to_string() == "urn:b"));
        assert!(!result.iter().any(|id| id.to_string() == "urn:a"));
        assert!(!result.iter().any(|id| id.to_string() == "urn:d"));
    }

    #[test]
    fn parse_pt5m() {
        let d = parse_iso8601_duration(Some("PT5M")).expect("duration");
        pretty_assertions::assert_eq!(d.to_string(), "PT5M");
    }

    #[test]
    fn parse_default_none() {
        let d = parse_iso8601_duration(None).expect("duration");
        pretty_assertions::assert_eq!(d.to_string(), "PT1H");
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_iso8601_duration(Some("invalid")).is_err());
    }

    #[test]
    fn parse_entity_map_urn_enforces_prefix() {
        let ok = parse_entity_map_urn("urn:ngsi-ld:EntityMap:1").expect("urn");
        pretty_assertions::assert_eq!(ok.as_str(), "urn:ngsi-ld:EntityMap:1");
        assert!(parse_entity_map_urn("urn:ngsi-ld:Snapshot:1").is_err());
    }

    #[test]
    fn parse_entity_map_urn_accepts_valid_urn() {
        let parsed = super::parse_entity_map_urn("urn:ngsi-ld:EntityMap:abc");
        assert!(parsed.is_ok());
    }
}
