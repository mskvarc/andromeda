//! One entity as it stood at the moment it was deleted.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// One entity as it stood at the moment it was deleted.
///
/// Carries the payload and system attributes the notification path needs,
/// which are no longer readable once the row is gone.
#[derive(Debug)]
pub struct DeletedEntity {
    /// Entity identifier.
    pub id: Uri,
    /// The expanded JSON-LD payload at the time of deletion.
    pub payload: Value,
    /// Timestamp when the entity was originally created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last modification before deletion.
    pub modified_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}
