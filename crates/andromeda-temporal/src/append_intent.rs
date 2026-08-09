//! Typed durable temporal append intents.

use crate::error::TemporalError;
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_id::InstanceId, scope::grammar::ScopeBuf, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use chrono::{DateTime, Utc};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Stable idempotency key of one committed temporal append intent.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]
#[display("{_0}")]
pub struct TemporalIntentId(Uuid);

impl TemporalIntentId {
    /// Mint one intent identifier at the anchor boundary.
    pub fn mint() -> TemporalIntentId {
        TemporalIntentId(Uuid::new_v4())
    }

    /// Construct an identifier decoded from durable storage.
    pub const fn from_uuid(value: Uuid) -> TemporalIntentId {
        TemporalIntentId(value)
    }

    /// Return the UUID used by adapters as an idempotency key.
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

/// Monotonic anchor-assigned order of committed intents within one tenant.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[display("{_0}")]
pub struct TemporalIntentSequence(i64);

impl TemporalIntentSequence {
    /// Construct a positive sequence decoded from durable storage.
    pub const fn new(value: i64) -> Result<TemporalIntentSequence, InvalidTemporalIntentSequence> {
        if value > 0 {
            Ok(TemporalIntentSequence(value))
        } else {
            Err(InvalidTemporalIntentSequence)
        }
    }

    /// Return the persistence representation.
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A durable store contained a non-positive temporal intent sequence.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[error("temporal intent sequence must be positive")]
pub struct InvalidTemporalIntentSequence;

/// Identity of one observation within an idempotent append intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TemporalObservationIdentity {
    /// Stable intent identifier.
    pub intent_id: TemporalIntentId,
    /// Zero-based position within the intent's ordered entries.
    pub ordinal: u32,
}

/// One typed entry in an ordered temporal append intent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TemporalAppendEntry {
    /// One neutral NGSI-LD attribute observation.
    Observation {
        /// Fully interpreted neutral history row.
        row: TemporalHistoryRow,
        /// Whether the client supplied the instance identifier.
        client_supplied_instance_id: bool,
        /// Server-side write-order timestamp shared by the intent.
        recorded_at: DateTime<Utc>,
    },
    /// End of one temporal attribute instance.
    Tombstone {
        /// Expanded attribute IRI.
        attribute_name: AttributeName,
        /// Multi-attribute discriminator, absent for the default instance.
        dataset_id: Option<Uri>,
        /// Stable deletion-marker instance identifier.
        instance_id: InstanceId,
        /// Temporal position of the deletion.
        recorded_at: DateTime<Utc>,
    },
    /// One scope observation used by temporal scope selection.
    Scope {
        /// Validated NGSI-LD scope path.
        path: ScopeBuf,
        /// Stable scope observation identifier.
        instance_id: InstanceId,
        /// Temporal position of the scope observation.
        recorded_at: DateTime<Utc>,
    },
}

/// One anchor-committed idempotent temporal append command.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TemporalAppendIntent {
    /// Stable sink idempotency key.
    pub id: TemporalIntentId,
    /// Entity whose temporal evolution receives the entries.
    pub entity_id: Uri,
    /// Entries in semantic write order.
    pub entries: Vec<TemporalAppendEntry>,
    /// Expanded observation payload used for pending stage-one selection.
    pub selection_value: Option<Value>,
}

impl TemporalAppendIntent {
    /// Interpret one landed atomic write's history consequences exactly once.
    pub fn from_history(entity_id: &Uri, history: &[HistoryRecord], recorded_at: DateTime<Utc>) -> Result<Option<TemporalAppendIntent>, TemporalError> {
        crate::intent_preparation::prepare(entity_id, history, recorded_at)
    }

    /// Iterate only attribute-history entries with their stable intent ordinals.
    pub fn history_entries(&self) -> impl Iterator<Item = (TemporalObservationIdentity, &TemporalAppendEntry)> {
        self.entries.iter().enumerate().filter_map(move |(position, entry)| {
            let ordinal = u32::try_from(position).ok()?;
            match entry {
                TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => {
                    Some((TemporalObservationIdentity { intent_id: self.id, ordinal }, entry))
                }
                TemporalAppendEntry::Scope { .. } => None,
            }
        })
    }
}

/// One committed intent together with its durable tenant-local order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SequencedTemporalAppendIntent {
    /// Monotonic commit sequence.
    pub sequence: TemporalIntentSequence,
    /// Idempotent append command.
    pub intent: TemporalAppendIntent,
}

#[cfg(test)]
mod tests {
    use crate::append_intent::{TemporalAppendEntry, TemporalAppendIntent};
    use andromeda_context_commit::history_record::HistoryRecord;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn generated_observation_identity_is_stable_inside_the_intent() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity ID");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
            }]
        }]));
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let intent = TemporalAppendIntent::from_history(&entity_id, &[HistoryRecord::Observations(payload)], recorded_at)
            .expect("valid history")
            .expect("non-empty intent");
        let TemporalAppendEntry::Observation { row, recorded_at: actual, .. } = &intent.entries[0] else {
            return;
        };
        pretty_assertions::assert_eq!(row.value, TemporalValue::Number(42.0));
        pretty_assertions::assert_eq!(*actual, recorded_at);
        assert!(row.instance_id.as_str().contains(&intent.id.to_string()));
    }
}
