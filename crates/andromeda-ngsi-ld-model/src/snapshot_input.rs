//! Input model for Snapshot creation.

use crate::{
    key_value_pair::KeyValuePair,
    query::body::Query,
    snapshot::{SnapshotPriority, SnapshotType},
};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use serde::{Deserialize, de::IgnoredAny};
use url::Url;

/// Input for `POST /snapshots`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotInput {
    /// Required fixed resource discriminator.
    #[serde(rename = "type")]
    pub type_: SnapshotType,
    /// Client-supplied identifier; the broker generates one when absent.
    #[serde(default)]
    pub id: Option<Identifier>,
    /// Entity queries to materialise.
    #[serde(default)]
    pub snapshot_queries: Option<Vec<Query>>,
    /// Temporal queries to materialise.
    #[serde(default)]
    pub snapshot_temporal_queries: Option<Vec<Query>>,
    /// How long the Snapshot should remain usable.
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// Requested execution priority.
    #[serde(default)]
    pub snapshot_priority: Option<SnapshotPriority>,
    /// Endpoint notified on terminal status and later metadata changes.
    #[serde(default)]
    pub endpoint: Option<Url>,
    /// Receiver metadata used by the configured notification binding.
    #[serde(default)]
    pub receiver_info: Option<Vec<KeyValuePair>>,
    /// Recognized output-only status, discarded on input.
    #[serde(default, rename = "snapshotStatus")]
    _snapshot_status: Option<IgnoredAny>,
    /// Recognized output-only entity execution details, discarded on input.
    #[serde(default, rename = "snapshotQueriesDetails")]
    _snapshot_queries_details: Option<IgnoredAny>,
    /// Recognized output-only temporal execution details, discarded on input.
    #[serde(default, rename = "snapshotTemporalQueriesDetails")]
    _snapshot_temporal_queries_details: Option<IgnoredAny>,
    /// Recognized output-only creation timestamp, discarded on input.
    #[serde(default)]
    _created_at: Option<IgnoredAny>,
    /// Recognized output-only modification timestamp, discarded on input.
    #[serde(default)]
    _modified_at: Option<IgnoredAny>,
    /// Recognized output-only expiry timestamp, discarded on input.
    #[serde(default)]
    _expires_at: Option<IgnoredAny>,
    /// Recognized output-only usage timestamp, discarded on input.
    #[serde(default)]
    _last_used_at: Option<IgnoredAny>,
}

#[cfg(test)]
mod tests {
    use crate::snapshot_input::SnapshotInput;
    use serde_json::json;

    #[test]
    fn known_output_members_are_accepted_and_discarded() {
        let input = serde_json::from_value::<SnapshotInput>(json!({
            "type": "Snapshot",
            "snapshotQueries": [],
            "snapshotStatus": "success",
            "snapshotQueriesDetails": [{"forged": true}],
            "snapshotTemporalQueriesDetails": [],
            "createdAt": "1900-01-01T00:00:00Z",
            "modifiedAt": null,
            "expiresAt": 12,
            "lastUsedAt": {"forged": true}
        }));
        assert!(input.is_ok());
    }

    #[test]
    fn unknown_members_are_rejected() {
        let input = serde_json::from_value::<SnapshotInput>(json!({"type": "Snapshot", "snapshotQueries": [], "unknown": true}));
        assert!(input.is_err());
    }
}
