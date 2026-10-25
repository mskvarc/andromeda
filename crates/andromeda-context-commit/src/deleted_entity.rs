//! One entity as it stood immediately before deletion.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::{DateTime, Utc};

/// System attributes no longer readable after deletion.
///
/// The payload is not among them: a deletion states its tombstones (clause
/// 5.6.6) from the entity's outline, and reads the payload it removes before it
/// runs only when a subscription may be owed its notification (clause 5.8.6),
/// so handing the payload back would decode it for nothing or a second time.
#[derive(Debug)]
pub struct DeletedEntity {
    /// Entity identifier.
    pub id: Identifier,
    /// Original creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp before deletion.
    pub modified_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::deleted_entity::DeletedEntity;
    use chrono::Utc;

    #[test]
    fn a_deleted_entity_retains_notification_state() {
        let now = Utc::now();
        let deleted = DeletedEntity {
            id: "urn:ngsi-ld:A".parse().expect("valid entity URI"),
            created_at: now,
            modified_at: now,
            expires_at: Some(now),
        };
        pretty_assertions::assert_eq!(deleted.created_at, now);
        pretty_assertions::assert_eq!(deleted.modified_at, now);
        pretty_assertions::assert_eq!(deleted.expires_at, Some(now));
    }
}
