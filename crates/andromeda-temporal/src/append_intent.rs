//! Typed durable temporal append intents.

use crate::{
    error::TemporalError,
    intent_preparation::prepare,
    scope_entry_kind::ScopeEntryKind,
    scope_observation::ScopeObservation,
    write_purpose::TemporalWritePurpose,
};
use andromeda_context_commit::{history_record::HistoryRecord, write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use chrono::{DateTime, Utc};
use derive_more::Display;

/// Monotonic anchor-assigned order of committed intents within one tenant.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct TemporalIntentKey(i64);

impl TemporalIntentKey {
    /// Construct a positive key decoded from durable storage.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTemporalIntentKey`] when `value` is not strictly
    /// positive: the queue numbers its commits from one, so zero or a negative
    /// value names no committed intent.
    pub const fn new(value: i64) -> Result<TemporalIntentKey, InvalidTemporalIntentKey> {
        if value > 0 {
            Ok(TemporalIntentKey(value))
        } else {
            Err(InvalidTemporalIntentKey)
        }
    }

    /// Return the persistence representation.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A durable store contained a non-positive temporal intent key.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[error("temporal intent key must be positive")]
pub struct InvalidTemporalIntentKey;

/// Identity of one observation within an idempotent append intent.
///
/// This is the pair a persisted history row deduplicates on, so a replay of the
/// same write reproduces it exactly and collides with the rows the first attempt
/// stored instead of duplicating them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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
        dataset_id: Option<Identifier>,
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
        /// Whether the entry is an event or an assertion of the Scope in force.
        kind: ScopeEntryKind,
        /// When the scope was observed (clause 4.5.6: the reified temporal
        /// scope carries `observedAt`); absent when the client declared none.
        observed_at: Option<DateTime<Utc>>,
        /// Stable scope observation identity.
        instance: InstanceIdentity,
    },
}

/// One anchor-committed idempotent temporal append command.
///
/// The intent is the unit the deferred queue holds and its drain applies,
/// so it owns the write identity and ingestion instant its rows are stored
/// under. Every entry shares them: they are consequences of one accepted write.
///
/// `base_ordinal` is the first ordinal this intent's entries occupy in its commit's
/// global consequence numbering. One commit's temporal consequences span every
/// landed entity, not just this one, so entries are numbered from `base_ordinal` rather
/// than from zero: that is what keeps two entities landed by the same commit
/// from ever numbering an entry with the same `(write_id, ordinal)` pair.
#[derive(Clone, Debug, PartialEq)]
pub struct TemporalAppendIntent {
    /// Identity and ingestion instant every entry of this intent is stored under.
    pub record: WriteRecord,
    /// First ordinal this intent's entries occupy in the commit's numbering.
    pub base_ordinal: WriteOrdinal,
    /// Entity whose temporal evolution receives the entries.
    pub entity_id: Identifier,
    /// Entity types the receiving Temporal Evolution is recorded under.
    ///
    /// Clause 4.5.6 makes the Temporal Evolution of an Entity "the Entity
    /// [...] as mandated by clause 4.5.1", so the evolution the entries extend
    /// carries the Entity's types and an intent that would bring one into being
    /// has to state them. They are the types of the Entity the write produced,
    /// not of the request fragment it was computed from: clauses 5.6.2.4,
    /// 5.6.3.4 and 5.6.17.4 each make `type` an optional Entity Fragment
    /// member, so a fragment names none while the Entity names several. Empty
    /// only when the write states no Entity at all, which is every deletion.
    pub entity_types: Vec<EntityTypeName>,
    /// Entries in semantic write order.
    pub entries: Vec<TemporalAppendEntry>,
}

impl TemporalAppendIntent {
    /// Interpret one landed atomic write's history consequences exactly once.
    ///
    /// `record` and `base_ordinal` come from the commit, not from this call: every
    /// entity a commit lands shares the commit's one write identity, and draws
    /// its entries' ordinals from the commit's single running counter.
    ///
    /// `entity_types` is what the write says the Entity is, which the history
    /// consequences alone cannot say: clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4
    /// each take an Entity Fragment whose `type` is optional, so the observed
    /// payload of an ordinary attribute write names no type while the Entity it
    /// landed in names several.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when a history record carries an
    /// instance a history row cannot record -- one carrying no value of the
    /// shape its family requires, an `observedAt` outside the clause 4.6.3 UTC
    /// form, or a Property value written in a time form it does not satisfy --
    /// and [`TemporalError::Internal`] when an observed entity state does not
    /// read back as the document it encodes to.
    pub fn from_history(
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        history: &[HistoryRecord],
        record: WriteRecord,
        base_ordinal: WriteOrdinal,
    ) -> Result<Option<TemporalAppendIntent>, TemporalError> {
        prepare(entity_id, entity_types, history, record, base_ordinal)
    }

    /// What this intent's entries are for.
    ///
    /// An intent whose every entry ends an instance records only endings: it is
    /// the temporal half of a deletion, and clauses 4.5.7 and 4.5.8 give it
    /// nothing to record when the Temporal Evolution it would have ended an
    /// instance of is not there. One carrying any observation or scope entry
    /// extends the evolution instead.
    ///
    /// An intent with no entries at all is an observation rather than a
    /// deletion. A deletion with no instance to end produces no history
    /// consequence and therefore no intent at all; what does reach here with no
    /// entries is a write that observed an Entity holding no Attribute, and
    /// clause 4.5.1 requires only `id` and `type` of an Entity while clause
    /// 4.5.6 gives that Entity a Temporal Evolution all the same — one its own
    /// write has to be able to establish.
    #[must_use]
    pub fn write_purpose(&self) -> TemporalWritePurpose {
        let ends_only = !self.entries.is_empty()
            && self.entries.iter().all(|entry| match entry {
                TemporalAppendEntry::Tombstone { .. } => true,
                TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Scope { .. } => false,
            });
        if ends_only {
            TemporalWritePurpose::Deletion
        } else {
            TemporalWritePurpose::Observation
        }
    }

    /// Iterate only attribute-history entries with their stable intent ordinals.
    pub fn history_entries(&self) -> impl Iterator<Item = (TemporalObservationIdentity, &TemporalAppendEntry)> {
        self.entries.iter().enumerate().filter_map(move |(position, entry)| {
            let ordinal = u32::try_from(position).ok().and_then(|delta| self.base_ordinal.advanced_by(delta).ok())?;
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

#[cfg(test)]
mod tests {
    use crate::{
        append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalObservationIdentity},
        scope_entry_kind::ScopeEntryKind,
        scope_observation::ScopeObservation,
        write_purpose::TemporalWritePurpose,
    };
    use andromeda_context_commit::{
        history_record::{HistoryRecord, tombstone},
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        entity_type_name::EntityTypeName,
        identifier::Identifier,
        instance_identity::InstanceIdentity,
        scope::path::ScopePath,
    };
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_number::JsonNumber;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";

    /// The clause 4.5.1 types a write states about the Entity it landed.
    fn vehicle_types() -> Vec<EntityTypeName> {
        vec![VEHICLE.parse::<EntityTypeName>().expect("valid entity type")]
    }

    /// The entity state an observation of `speed` = 42 records.
    fn speed_payload(entity_id: &Identifier) -> Arc<EntityState> {
        state(&json!([{
            "@id": entity_id,
            "@type": [VEHICLE],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
            }]
        }]))
    }

    /// Decode a stored entity document.
    fn state(document: &serde_json::Value) -> Arc<EntityState> {
        Arc::new(EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes"))
    }

    #[test]
    fn generated_observation_identity_is_stable_inside_the_intent() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let payload = speed_payload(&entity_id);
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let intent = TemporalAppendIntent::from_history(
            &entity_id,
            &vehicle_types(),
            &[HistoryRecord::Observations(payload)],
            record,
            WriteOrdinal::at(0),
        )
        .expect("valid history")
        .expect("non-empty intent");
        let TemporalAppendEntry::Observation { row } = &intent.entries[0] else {
            return;
        };
        pretty_assertions::assert_eq!(row.value, TemporalValue::Number(JsonNumber::from(42)));
        pretty_assertions::assert_eq!(intent.record.recorded_at(), recorded_at);
        pretty_assertions::assert_eq!(row.recorded_at, recorded_at);
        assert!(matches!(row.instance, InstanceIdentity::Generated(_)));
    }

    /// Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated
    /// by clause 4.5.1", so the intent records the types the write states about
    /// the Entity. The observed payload here is an Entity Fragment carrying one
    /// attribute and, as clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4 all permit, no
    /// `type` at all: reading the types off it would say the Entity has none.
    #[test]
    fn an_intent_carries_the_entity_types_the_write_stated_not_the_fragments() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let payload = speed_payload(&entity_id);
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let intent = TemporalAppendIntent::from_history(
            &entity_id,
            &vehicle_types(),
            &[HistoryRecord::Observations(payload)],
            record,
            WriteOrdinal::at(0),
        )
        .expect("valid history")
        .expect("non-empty intent");
        pretty_assertions::assert_eq!(intent.entity_types, vehicle_types());
        pretty_assertions::assert_eq!(intent.write_purpose(), TemporalWritePurpose::Observation);
    }

    /// Clause 5.6.16 lets a client delete the Temporal Evolution of an Entity
    /// while clause 5.6.6 deletes current state, each on its own, so the
    /// deletion an Entity delete records can meet an Entity that no longer has
    /// an evolution. An intent that only ends instances says so, and nothing
    /// below it has any other way to tell.
    #[test]
    fn an_intent_of_only_tombstones_is_a_deletion() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let speed = "https://example.com/speed".parse::<AttributeName>().expect("valid attribute name");
        let colour = "https://example.com/colour".parse::<AttributeName>().expect("valid attribute name");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let history = [
            tombstone(&colour, None, AttributeType::Property),
            tombstone(&speed, None, AttributeType::Property),
        ];
        let intent = TemporalAppendIntent::from_history(
            &entity_id,
            &[],
            &history,
            WriteRecord::restored(WriteId::mint(), recorded_at),
            WriteOrdinal::at(0),
        )
        .expect("valid history")
        .expect("the tombstones produce an intent");
        pretty_assertions::assert_eq!(intent.write_purpose(), TemporalWritePurpose::Deletion);
    }

    /// A write that observes anything extends the evolution, so it needs one:
    /// clause 4.5.1 makes at least one Entity type part of what an Entity is,
    /// and an observation stating none cannot bring an evolution into being.
    #[test]
    fn an_intent_carrying_an_observation_is_an_observation_even_beside_a_tombstone() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let colour = "https://example.com/colour".parse::<AttributeName>().expect("valid attribute name");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let history = [
            HistoryRecord::Observations(speed_payload(&entity_id)),
            tombstone(&colour, None, AttributeType::Property),
        ];
        let intent = TemporalAppendIntent::from_history(
            &entity_id,
            &vehicle_types(),
            &history,
            WriteRecord::restored(WriteId::mint(), recorded_at),
            WriteOrdinal::at(0),
        )
        .expect("valid history")
        .expect("the payload produces an intent");
        pretty_assertions::assert_eq!(intent.write_purpose(), TemporalWritePurpose::Observation);
    }

    /// Clause 4.5.1 requires only `id` and `type` of an Entity, so a create
    /// that carries no Attribute observes one all the same and clause 4.5.6
    /// still gives it a Temporal Evolution. A deletion with nothing to end
    /// produces no history consequence and so no intent, which is what leaves
    /// an entry-less intent unambiguously an observation.
    #[test]
    fn an_intent_with_no_entries_at_all_is_an_observation() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let attributeless = state(&json!([{"@id": entity_id, "@type": [VEHICLE]}]));
        let intent = TemporalAppendIntent::from_history(
            &entity_id,
            &vehicle_types(),
            &[HistoryRecord::Observations(attributeless)],
            WriteRecord::restored(WriteId::mint(), recorded_at),
            WriteOrdinal::at(0),
        )
        .expect("valid history")
        .expect("an observation produces an intent even with nothing to record");
        pretty_assertions::assert_eq!(intent.entries, Vec::new());
        pretty_assertions::assert_eq!(intent.write_purpose(), TemporalWritePurpose::Observation);
    }

    /// Clause 4.5.6 represents the Scope of a Temporal Evolution as a temporal
    /// Property of its own, so a scope entry extends the evolution exactly as an
    /// attribute observation does, deleted scope included.
    #[test]
    fn an_intent_carrying_only_a_scope_entry_is_an_observation() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let intent = TemporalAppendIntent {
            record: WriteRecord::restored(WriteId::mint(), recorded_at),
            base_ordinal: WriteOrdinal::at(0),
            entity_types: Vec::new(),
            entity_id,
            entries: vec![TemporalAppendEntry::Scope {
                observation: ScopeObservation::Path("/Madrid".parse::<ScopePath>().expect("a valid scope path")),
                kind: ScopeEntryKind::Event,
                observed_at: Some(recorded_at),
                instance: InstanceIdentity::generate(),
            }],
        };
        pretty_assertions::assert_eq!(intent.write_purpose(), TemporalWritePurpose::Observation);
    }

    /// The commit that produced this intent may have landed other entities
    /// first, so their entries already consumed the low ordinals; this
    /// intent's own entries must be numbered starting from the base it was
    /// handed rather than from zero.
    #[test]
    fn history_entries_number_from_the_intents_base_rather_than_zero() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity ID");
        let payload = speed_payload(&entity_id);
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let base = WriteOrdinal::at(5);
        let intent = TemporalAppendIntent::from_history(&entity_id, &vehicle_types(), &[HistoryRecord::Observations(payload)], record, base)
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
