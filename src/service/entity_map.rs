use crate::{domain::entity_map::EntityMap, error::AndromedaError, repository::entity_map::EntityMapStore};
use chrono::{DateTime, Duration, Utc};

/// Service for managing NGSI-LD entity maps.
pub struct EntityMapService {
    store: EntityMapStore,
}

impl EntityMapService {
    /// Create a new entity map service backed by the given store.
    pub const fn new(store: EntityMapStore) -> EntityMapService {
        EntityMapService { store }
    }

    /// Create a new entity map from the given entity IDs with an optional ISO 8601 lifetime.
    pub async fn create(&self, entity_ids: Vec<String>, lifetime: Option<&str>) -> Result<EntityMap, AndromedaError> {
        let id = format!("urn:ngsi-ld:EntityMap:{}", uuid::Uuid::new_v4());
        let duration = parse_iso8601_duration(lifetime)?;
        let expires_at = Utc::now() + duration;
        let map = EntityMap::new(id, expires_at, entity_ids);
        self.store.insert(map.clone()).await; // clone: store takes ownership, caller needs return value
        Ok(map)
    }

    /// Retrieve an entity map by its identifier.
    pub async fn retrieve(&self, id: &str) -> Result<EntityMap, AndromedaError> {
        self.store.get(id).await.ok_or_else(|| AndromedaError::not_found(id))
    }

    /// Update the expiration time of an existing entity map.
    pub async fn update(&self, id: &str, expires_at: DateTime<Utc>) -> Result<(), AndromedaError> {
        if self.store.update(id, expires_at).await {
            Ok(())
        } else {
            Err(AndromedaError::not_found(id))
        }
    }

    /// Scope query IDs by intersecting with an EntityMap's entity set.
    ///
    /// When `existing_ids` is `Some`, returns only the IDs present in both the
    /// map and the existing set. When `None`, returns all IDs from the map as
    /// a comma-separated string. Per spec 5.5.9.3, this scopes queries to the
    /// frozen entity set of the given map.
    pub async fn scope_query_ids(&self, map_id: &str, existing_ids: Option<&str>) -> Result<String, AndromedaError> {
        let map = self.retrieve(map_id).await?;
        if let Some(existing) = existing_ids {
            let query_ids: std::collections::HashSet<String> = existing.split(',').map(|s| s.trim().to_string()).collect();
            let map_ids: std::collections::HashSet<String> = map.entity_ids.into_iter().collect();
            let intersection: Vec<String> = query_ids.intersection(&map_ids).cloned().collect(); // clone: owned strings from set intersection
            Ok(intersection.join(","))
        } else {
            Ok(map.entity_ids.join(","))
        }
    }

    /// Validate that a specific entity ID exists in the given EntityMap.
    ///
    /// Returns `Ok(())` if the entity is present, or `Err(NotFound)` if it is
    /// not. Per spec 5.7.3.4, entity-map-scoped retrieval must return 404 for
    /// entities not in the map.
    pub async fn validate_entity_in_map(&self, map_id: &str, entity_id: &str) -> Result<(), AndromedaError> {
        let map = self.retrieve(map_id).await?;
        if map.entity_ids.iter().any(|id| id == entity_id) {
            Ok(())
        } else {
            Err(AndromedaError::not_found(entity_id))
        }
    }

    /// Delete an entity map by its identifier.
    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        if self.store.delete(id).await {
            Ok(())
        } else {
            Err(AndromedaError::not_found(id))
        }
    }
}

/// Parse an ISO 8601 duration string (e.g. `PT5M`, `P1D`) into a `chrono::Duration`.
///
/// Returns a default of 1 hour when `input` is `None` or empty.
pub fn parse_iso8601_duration(input: Option<&str>) -> Result<Duration, AndromedaError> {
    let Some(s) = input else {
        return Ok(Duration::hours(1));
    };

    if s.is_empty() {
        return Ok(Duration::hours(1));
    }

    // Simple ISO 8601 duration parser for PT#H#M#S format
    let s = s.trim();
    if !s.starts_with('P') {
        return Err(AndromedaError::bad_request_data(format!("Invalid ISO 8601 duration: {}", s)));
    }

    let rest = &s[1..];
    let (date_part, time_part) = if let Some(idx) = rest.find('T') {
        (&rest[..idx], &rest[idx + 1..])
    } else {
        (rest, "")
    };

    let mut total_seconds: i64 = 0;

    // Parse date part (D for days)
    if !date_part.is_empty() {
        let mut num_buf = String::new();
        for c in date_part.chars() {
            if c.is_ascii_digit() {
                num_buf.push(c);
            } else {
                let val: i64 = num_buf
                    .parse()
                    .map_err(|_| AndromedaError::bad_request_data(format!("Invalid ISO 8601 duration: {}", s)))?;
                num_buf.clear();
                match c {
                    'D' => total_seconds += val * 86400,
                    _other => {
                        return Err(AndromedaError::bad_request_data(format!("Invalid ISO 8601 duration: {}", s)));
                    }
                }
            }
        }
    }

    // Parse time part (H, M, S)
    if !time_part.is_empty() {
        let mut num_buf = String::new();
        for c in time_part.chars() {
            if c.is_ascii_digit() || c == '.' {
                num_buf.push(c);
            } else {
                let val: f64 = num_buf
                    .parse()
                    .map_err(|_| AndromedaError::bad_request_data(format!("Invalid ISO 8601 duration: {}", s)))?;
                num_buf.clear();
                match c {
                    'H' => total_seconds += (val * 3600.0) as i64,
                    'M' => total_seconds += (val * 60.0) as i64,
                    'S' => total_seconds += val as i64,
                    _other => {
                        return Err(AndromedaError::bad_request_data(format!("Invalid ISO 8601 duration: {}", s)));
                    }
                }
            }
        }
    }

    if total_seconds == 0 {
        return Err(AndromedaError::bad_request_data(format!("Invalid ISO 8601 duration: {}", s)));
    }

    Ok(Duration::seconds(total_seconds))
}

#[cfg(test)]
mod tests {
    use crate::{repository::entity_map::EntityMapStore, service::entity_map::parse_iso8601_duration};

    fn make_service() -> super::EntityMapService {
        super::EntityMapService::new(EntityMapStore::new())
    }

    #[tokio::test]
    async fn scope_query_ids_no_existing_returns_all_map_ids() {
        let svc = make_service();
        let map = svc.create(vec!["urn:a".to_string(), "urn:b".to_string()], None).await.unwrap();
        let result = svc.scope_query_ids(&map.id, None).await.unwrap();
        assert!(result.contains("urn:a"));
        assert!(result.contains("urn:b"));
    }

    #[tokio::test]
    async fn scope_query_ids_with_overlap_returns_intersection() {
        let svc = make_service();
        let map = svc
            .create(vec!["urn:a".to_string(), "urn:b".to_string(), "urn:c".to_string()], None)
            .await
            .unwrap();
        let result = svc.scope_query_ids(&map.id, Some("urn:b,urn:d")).await.unwrap();
        assert!(result.contains("urn:b"));
        assert!(!result.contains("urn:a"));
        assert!(!result.contains("urn:d"));
    }

    #[tokio::test]
    async fn scope_query_ids_no_overlap_returns_empty() {
        let svc = make_service();
        let map = svc.create(vec!["urn:a".to_string()], None).await.unwrap();
        let result = svc.scope_query_ids(&map.id, Some("urn:x")).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn scope_query_ids_map_not_found_returns_error() {
        let svc = make_service();
        let result = svc.scope_query_ids("urn:nonexistent", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn validate_entity_in_map_id_present() {
        let svc = make_service();
        let map = svc.create(vec!["urn:a".to_string(), "urn:b".to_string()], None).await.unwrap();
        assert!(svc.validate_entity_in_map(&map.id, "urn:a").await.is_ok());
    }

    #[tokio::test]
    async fn validate_entity_in_map_id_absent_returns_not_found() {
        let svc = make_service();
        let map = svc.create(vec!["urn:a".to_string()], None).await.unwrap();
        assert!(svc.validate_entity_in_map(&map.id, "urn:x").await.is_err());
    }

    #[test]
    fn parse_pt5m() {
        let d = parse_iso8601_duration(Some("PT5M")).unwrap();
        assert_eq!(d.num_seconds(), 300);
    }

    #[test]
    fn parse_pt1h() {
        let d = parse_iso8601_duration(Some("PT1H")).unwrap();
        assert_eq!(d.num_seconds(), 3600);
    }

    #[test]
    fn parse_pt1h30m() {
        let d = parse_iso8601_duration(Some("PT1H30M")).unwrap();
        assert_eq!(d.num_seconds(), 5400);
    }

    #[test]
    fn parse_p1d() {
        let d = parse_iso8601_duration(Some("P1D")).unwrap();
        assert_eq!(d.num_seconds(), 86400);
    }

    #[test]
    fn parse_default_none() {
        let d = parse_iso8601_duration(None).unwrap();
        assert_eq!(d.num_seconds(), 3600);
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_iso8601_duration(Some("invalid")).is_err());
    }
}
