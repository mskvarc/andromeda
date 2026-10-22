//! Input model for asynchronous Snapshot cloning.

use crate::{
    key_value_pair::KeyValuePair,
    snapshot::{SnapshotPriority, SnapshotType},
};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use serde::Deserialize;
use url::Url;

/// Input for `POST /snapshots/{id}/clone`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotCloneInput {
    /// Required fixed resource discriminator.
    #[serde(rename = "type")]
    pub type_: SnapshotType,
    /// Optional target identifier.
    #[serde(default)]
    pub id: Option<Identifier>,
    /// Optional target lifetime.
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// Optional target priority.
    #[serde(default)]
    pub snapshot_priority: Option<SnapshotPriority>,
    /// Optional target notification endpoint.
    #[serde(default)]
    pub endpoint: Option<Url>,
    /// Optional target receiver metadata.
    #[serde(default)]
    pub receiver_info: Option<Vec<KeyValuePair>>,
}

#[cfg(test)]
mod tests {
    use crate::snapshot_clone::SnapshotCloneInput;
    use serde_json::json;

    #[test]
    fn clone_rejects_query_and_execution_detail_members() {
        for body in [
            json!({"type": "Snapshot", "snapshotQueries": []}),
            json!({"type": "Snapshot", "snapshotTemporalQueries": []}),
            json!({"type": "Snapshot", "snapshotQueriesDetails": []}),
        ] {
            assert!(serde_json::from_value::<SnapshotCloneInput>(body).is_err());
        }
    }
}
