//! The Snapshot Fragment of an Update Snapshot Status (clause 5.16.4).
//!
//! Clause 5.16.4.4 modifies the target Snapshot "as mandated by clause 5.5.8",
//! so a member written as the NGSI-LD Null is deleted and a JSON `null` is no
//! value of any member (clause 5.2.1).

use crate::{
    clearable::Clearable,
    clearable_array,
    key_value_pair::KeyValuePair,
    snapshot::{SnapshotPriority, SnapshotType},
};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use serde::{Deserialize, de::IgnoredAny};
use url::Url;

/// Input for `PATCH /snapshots/{id}`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotPatch {
    /// Optional exact Snapshot discriminator.
    #[serde(default, rename = "type")]
    pub type_: Option<SnapshotType>,
    /// Optional unchanged identifier assertion.
    #[serde(default)]
    pub id: Option<Identifier>,
    /// New positive lifetime.
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// New priority.
    #[serde(default)]
    pub snapshot_priority: Option<SnapshotPriority>,
    /// Replacement endpoint, or its deletion.
    #[serde(default)]
    pub endpoint: Clearable<Url>,
    /// Replacement receiver metadata, or its deletion.
    #[serde(default, deserialize_with = "clearable_array::deserialize")]
    pub receiver_info: Clearable<Vec<KeyValuePair>>,
    /// Query definitions are recognized so policy can reject attempts to mutate them.
    #[serde(default)]
    pub snapshot_queries: Clearable<IgnoredAny>,
    /// Temporal query definitions are recognized so policy can reject attempts to mutate them.
    #[serde(default)]
    pub snapshot_temporal_queries: Clearable<IgnoredAny>,
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{clearable::Clearable, request_body::RequestBody, snapshot_patch::SnapshotPatch};
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use serde_json::{Value, json};

    fn read(fragment: &Value) -> Result<SnapshotPatch, serde_json::Error> {
        SnapshotPatch::from_request_text(fragment.to_string().as_bytes(), ContextReading::keyword_spellings())
    }

    /// Clause 5.5.8: an omitted member is left alone and the NGSI-LD Null
    /// deletes one, an array member also as an array holding the Null alone.
    #[test]
    fn omission_and_the_ngsi_ld_null_are_distinct_for_removable_members() {
        let omitted = read(&json!({})).expect("empty fragment is valid");
        assert!(omitted.endpoint.is_absent() && omitted.receiver_info.is_absent());
        for receiver_info in [json!("urn:ngsi-ld:null"), json!(["urn:ngsi-ld:null"])] {
            let removed = read(&json!({"endpoint": "urn:ngsi-ld:null", "receiverInfo": receiver_info})).expect("removal fragment is valid");
            assert!(removed.endpoint.is_ngsi_ld_null() && removed.receiver_info.is_ngsi_ld_null());
        }
        let set = read(&json!({"endpoint": "https://receiver.example.com/notify"})).expect("a replacement");
        assert!(matches!(set.endpoint, Clearable::Set(_)));
    }

    /// Clause 5.2.1: a JSON `null` is no value of any member.
    #[test]
    fn a_json_null_is_refused_on_every_member() {
        for member in [
            "snapshotLifetime",
            "snapshotPriority",
            "type",
            "id",
            "endpoint",
            "receiverInfo",
            "snapshotQueries",
            "createdAt",
        ] {
            assert!(read(&json!({ member: null })).is_err(), "{member}");
        }
    }

    /// Members that cannot be deleted refuse the NGSI-LD Null as a value
    /// outside their data type.
    #[test]
    fn the_ngsi_ld_null_on_a_member_that_cannot_be_deleted_is_refused() {
        for member in ["snapshotLifetime", "snapshotPriority", "type"] {
            assert!(read(&json!({ member: "urn:ngsi-ld:null" })).is_err(), "{member}");
        }
    }
}
