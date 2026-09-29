//! Merge-patch model for Snapshot metadata updates.

use crate::{
    key_value_pair::KeyValuePair,
    snapshot::{SnapshotPriority, SnapshotType},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use url::Url;

/// Presence-aware JSON merge-patch member.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MergePatchValue<T> {
    /// The member was omitted and must remain unchanged.
    #[default]
    Absent,
    /// The member was present as JSON null and must be removed.
    Null,
    /// The member was present with a replacement value.
    Value(T),
}

impl<'de, T> Deserialize<'de> for MergePatchValue<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<MergePatchValue<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(|value| match value {
            Some(value) => MergePatchValue::Value(value),
            None => MergePatchValue::Null,
        })
    }
}

/// Deserializes a required non-null value wrapped for an optional patch member.
fn deserialize_present_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Input for `PATCH /snapshots/{id}`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotPatch {
    /// Optional exact Snapshot discriminator; JSON null and other types are invalid.
    #[serde(default, rename = "type", deserialize_with = "deserialize_present_non_null")]
    pub type_: Option<SnapshotType>,
    /// Optional unchanged identifier assertion.
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    pub id: Option<Identifier>,
    /// New positive lifetime. JSON null is invalid.
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// New priority. JSON null is invalid.
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    pub snapshot_priority: Option<SnapshotPriority>,
    /// Replacement endpoint or JSON null removal.
    #[serde(default)]
    pub endpoint: MergePatchValue<Url>,
    /// Replacement receiver metadata or JSON null removal.
    #[serde(default)]
    pub receiver_info: MergePatchValue<Vec<KeyValuePair>>,
    /// Query definitions are recognized so policy can reject attempts to mutate them.
    #[serde(default)]
    pub snapshot_queries: MergePatchValue<IgnoredAny>,
    /// Temporal query definitions are recognized so policy can reject attempts to mutate them.
    #[serde(default)]
    pub snapshot_temporal_queries: MergePatchValue<IgnoredAny>,
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
    use crate::snapshot_patch::{MergePatchValue, SnapshotPatch};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn omission_and_null_are_distinct_for_removable_members() {
        let omitted = serde_json::from_value::<SnapshotPatch>(json!({})).expect("empty fragment is valid");
        let removed = serde_json::from_value::<SnapshotPatch>(json!({"endpoint": null, "receiverInfo": null})).expect("removal fragment is valid");
        assert_eq!(omitted.endpoint, MergePatchValue::Absent);
        assert_eq!(removed.endpoint, MergePatchValue::Null);
        assert_eq!(removed.receiver_info, MergePatchValue::Null);
    }

    #[test]
    fn null_lifetime_priority_and_type_are_rejected() {
        for fragment in [json!({"snapshotLifetime": null}), json!({"snapshotPriority": null}), json!({"type": null})] {
            assert!(serde_json::from_value::<SnapshotPatch>(fragment).is_err());
        }
    }
}
