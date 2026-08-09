//! One entity as it stood immediately before deletion.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Payload and system attributes no longer readable after deletion.
#[derive(Debug)]
pub struct DeletedEntity {
    /// Entity identifier.
    pub id: Uri,
    /// Expanded JSON-LD payload at deletion time.
    pub payload: Value,
    /// Original creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp before deletion.
    pub modified_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use crate::deleted_entity::DeletedEntity;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn a_deleted_entity_retains_notification_state() {
        let now = Utc::now();
        let deleted = DeletedEntity {
            id: "urn:ngsi-ld:A".parse().expect("valid entity URI"),
            payload: json!([{"@id": "urn:ngsi-ld:A"}]),
            created_at: now,
            modified_at: now,
            expires_at: Some(now),
        };
        pretty_assertions::assert_eq!(deleted.created_at, now);
        pretty_assertions::assert_eq!(deleted.modified_at, now);
        pretty_assertions::assert_eq!(deleted.expires_at, Some(now));
    }
}
