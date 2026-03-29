use crate::{domain::entity_map::EntityMap, error::AndromedaError, repository::entity_map::EntityMapStore};
use chrono::{DateTime, Duration, Utc};

pub struct EntityMapService {
    store: EntityMapStore,
}

impl EntityMapService {
    pub fn new(store: EntityMapStore) -> Self {
        Self { store }
    }

    pub async fn create(&self, entity_ids: Vec<String>, lifetime: Option<&str>) -> Result<EntityMap, AndromedaError> {
        let id = format!("urn:ngsi-ld:EntityMap:{}", uuid::Uuid::new_v4());
        let duration = parse_iso8601_duration(lifetime)?;
        let expires_at = Utc::now() + duration;
        let map = EntityMap::new(id, expires_at, entity_ids);
        self.store.insert(map.clone()).await;
        Ok(map)
    }

    pub async fn retrieve(&self, id: &str) -> Result<EntityMap, AndromedaError> {
        self.store.get(id).await.ok_or_else(|| AndromedaError::not_found(id))
    }

    pub async fn update(&self, id: &str, expires_at: DateTime<Utc>) -> Result<(), AndromedaError> {
        if self.store.update(id, expires_at).await {
            Ok(())
        } else {
            Err(AndromedaError::not_found(id))
        }
    }

    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        if self.store.delete(id).await {
            Ok(())
        } else {
            Err(AndromedaError::not_found(id))
        }
    }
}

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
                    _ => {
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
                    _ => {
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
    use super::*;

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
