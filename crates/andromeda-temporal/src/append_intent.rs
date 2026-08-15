//! Typed durable temporal append intents.

use crate::{error::TemporalError, intent_preparation::prepare, scope_observation::ScopeObservation};
use andromeda_context_commit::{history_record::HistoryRecord, write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_identity::InstanceIdentity, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
///
/// This is the pair a persisted history row deduplicates on, so a replay of the
/// same write reproduces it exactly and collides with the rows the first attempt
/// stored instead of duplicating them.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TemporalObservationIdentity {
    /// The write whose temporal consequences the observation belongs to.
    pub write_id: WriteId,
    /// Zero-based position within the intent's ordered entries.
    pub ordinal: WriteOrdinal,
}

/// One typed entry in an ordered temporal append intent.
///
/// No entry carries an instant of its own: every consequence of one accepted
/// write shares the write's ingestion instant, which the intent holds once.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TemporalAppendEntry {
    /// One neutral NGSI-LD attribute observation.
    Observation {
        /// Fully interpreted neutral history row.
        row: TemporalHistoryRow,
    },
    /// End of one temporal attribute instance.
    Tombstone {
        /// Expanded attribute IRI.
        attribute_name: AttributeName,
        /// Multi-attribute discriminator, absent for the default instance.
        dataset_id: Option<Uri>,
        /// Attribute family the ended instance belonged to.
        ///
        /// Clauses 4.5.7 and 4.5.8 render a deleted instance with the
        /// `urn:ngsi-ld:null` member of its own family, so the family is part of
        /// the deletion rather than something the sink re-derives from a
        /// catalogue a later type change could have overwritten.
        attribute_type: AttributeType,
        /// Stable deletion-marker instance identity.
        instance: InstanceIdentity,
    },
    /// One scope observation used by temporal scope selection.
    Scope {
        /// What the observation records about the entity's scope.
        observation: ScopeObservation,
        /// Stable scope observation identity.
        instance: InstanceIdentity,
    },
}

/// One anchor-committed idempotent temporal append command.
///
/// The intent is the unit the deferred queue claims, acknowledges and replays,
/// so it owns the write identity and ingestion instant its rows are stored
/// under. Every entry shares them: they are consequences of one accepted write.
///
/// `base` is the first ordinal this intent's entries occupy in its commit's
/// global consequence numbering. One commit's temporal consequences span every
/// landed entity, not just this one, so entries are numbered from `base` rather
/// than from zero: that is what keeps two entities landed by the same commit
/// from ever numbering an entry with the same `(write_id, ordinal)` pair.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TemporalAppendIntent {
    /// Identity and ingestion instant every entry of this intent is stored under.
    pub record: WriteRecord,
    /// First ordinal this intent's entries occupy in the commit's numbering.
    pub base: WriteOrdinal,
    /// Entity whose temporal evolution receives the entries.
    pub entity_id: Uri,
    /// Entries in semantic write order.
    pub entries: Vec<TemporalAppendEntry>,
    /// Expanded observation payload used for pending stage-one selection.
    pub selection_value: Option<Value>,
}

impl TemporalAppendIntent {
    /// Interpret one landed atomic write's history consequences exactly once.
    ///
    /// `record` and `base` come from the commit, not from this call: every
    /// entity a commit lands shares the commit's one write identity, and draws
    /// its entries' ordinals from the commit's single running counter.
    pub fn from_history(
        entity_id: &Uri,
        history: &[HistoryRecord],
        record: WriteRecord,
        base: WriteOrdinal,
    ) -> Result<Option<TemporalAppendIntent>, TemporalError> {
        prepare(entity_id, history, record, base)
    }

    /// Iterate only attribute-history entries with their stable intent ordinals.
    pub fn history_entries(&self) -> impl Iterator<Item = (TemporalObservationIdentity, &TemporalAppendEntry)> {
        self.entries.iter().enumerate().filter_map(move |(position, entry)| {
            let ordinal = u32::try_from(position).ok().and_then(|delta| self.base.advanced_by(delta).ok())?;
            match entry {
                TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => Some((
                    TemporalObservationIdentity {
                        write_id: self.record.write_id(),
                        ordinal,
                    },
                    entry,
                )),
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
    use crate::append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalObservationIdentity};
    use andromeda_context_commit::{history_record::HistoryRecord, write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_ngsi_ld_lexicon::{instance_identity::InstanceIdentity, uri::Uri};
    use andromeda_ngsi_ld_model::{attribute::json_number::JsonNumber, temporal::TemporalValue};
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    fn speed_payload(entity_id: &Uri) -> Arc<serde_json::Value> {
        Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
            }]
        }]))
    }

    #[test]
    fn generated_observation_identity_is_stable_inside_the_intent() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity ID");
        let payload = speed_payload(&entity_id);
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let intent = TemporalAppendIntent::from_history(&entity_id, &[HistoryRecord::Observations(payload)], record, WriteOrdinal::at(0))
            .expect("valid history")
            .expect("non-empty intent");
        let TemporalAppendEntry::Observation { row } = &intent.entries[0] else {
            return;
        };
        pretty_assertions::assert_eq!(row.value, TemporalValue::Number(JsonNumber::Signed(42)));
        pretty_assertions::assert_eq!(intent.record.recorded_at(), recorded_at);
        pretty_assertions::assert_eq!(row.recorded_at, recorded_at);
        assert!(matches!(row.instance, InstanceIdentity::Generated(_)));
    }

    /// The commit that produced this intent may have landed other entities
    /// first, so their entries already consumed the low ordinals; this
    /// intent's own entries must be numbered starting from the base it was
    /// handed rather than from zero.
    #[test]
    fn history_entries_number_from_the_intents_base_rather_than_zero() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity ID");
        let payload = speed_payload(&entity_id);
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let base = WriteOrdinal::at(5);
        let intent = TemporalAppendIntent::from_history(&entity_id, &[HistoryRecord::Observations(payload)], record, base)
            .expect("valid history")
            .expect("non-empty intent");
        let (identity, _entry) = intent.history_entries().next().expect("one history entry");
        pretty_assertions::assert_eq!(
            identity,
            TemporalObservationIdentity {
                write_id: record.write_id(),
                ordinal: WriteOrdinal::at(5),
            }
        );
    }
}
