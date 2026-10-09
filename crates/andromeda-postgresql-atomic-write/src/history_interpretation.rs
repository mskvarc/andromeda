//! The one interpretation of a commit's temporal consequences.
//!
//! Every landed write's history is interpreted into a temporal append intent
//! exactly once. A Core write's history is the current-state change it made
//! (clause 4.3.1), which [`CoreWriteHistory::NotRecorded`] leaves out of the
//! Temporal Evolution; a Temporal API write's history is every instance the
//! client sent (clauses 5.6.11.4 and 5.6.12.4), recorded whatever that setting
//! says.

use crate::{attempt_failure::AttemptFailure, commit_refusal::temporal};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_invariant::CommitInvariant,
    error::CommitError,
    history_record::HistoryRecord,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_temporal::{append_intent::TemporalAppendIntent, core_write_history::CoreWriteHistory, error::TemporalError};

/// Interpret every landed write's temporal consequences exactly once, and
/// report the ordinal one past the last one this phase consumed.
///
/// Every entity a commit lands shares the commit's one write identity, so
/// their intents cannot each restart their own ordinal count at zero: every
/// intent draws its entries' ordinals from the same running counter, seeded at
/// `base`, and the counter advances past exactly the entries that intent used.
///
/// Under [`CoreWriteHistory::NotRecorded`] a Core write, one whose history
/// holds no instance added through the Temporal API, records nothing and draws
/// no ordinal.
///
/// A refusal raised while interpreting one write's history names that write's
/// content alone, so it is attributed to the write it came from.
pub(crate) fn interpret(
    landed: &[&AtomicWrite],
    record: &WriteRecord,
    base: WriteOrdinal,
    core_write_history: CoreWriteHistory,
) -> Result<(Vec<TemporalAppendIntent>, WriteOrdinal), AttemptFailure> {
    let mut next = base;
    let mut intents = Vec::with_capacity(landed.len());
    for write in landed {
        let core_write = !write.history.iter().any(HistoryRecord::is_evolution_instances);
        match (core_write_history, core_write) {
            (CoreWriteHistory::NotRecorded, true) => continue,
            (CoreWriteHistory::NotRecorded, false) | (CoreWriteHistory::Recorded, true | false) => {}
        }
        let refused = |error: TemporalError| AttemptFailure::of_write(temporal(error), Identifier::clone(&write.entity_id)); // clone: the attribution owns the entity it names.
        let Some(intent) =
            TemporalAppendIntent::from_history(&write.entity_id, &write.recorded_entity_types(), &write.history, *record, next).map_err(refused)?
        else {
            continue;
        };
        let advance = u32::try_from(intent.entries.len()).map_err(|error| CommitError::from(CommitInvariant::ConsequenceCount(error)))?;
        next = next
            .advanced_by(advance)
            .map_err(|error| CommitError::from(CommitInvariant::OrdinalOverflow(error)))?;
        intents.push(intent);
    }
    Ok((intents, next))
}

#[cfg(test)]
mod tests {
    use crate::history_interpretation::interpret;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        history_record::{HistoryRecord, tombstone},
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
    use andromeda_temporal::core_write_history::CoreWriteHistory;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Arc;

    /// The stored expanded document of a Vehicle whose `speed` is 1.
    fn speed_document(entity_id: &str) -> Vec<u8> {
        serde_json::to_vec(&json!([{
            "@id": entity_id,
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            }]
        }]))
        .expect("the fixture serialises")
    }

    fn speed(entity_id: &str) -> Arc<EntityState> {
        Arc::new(EntityState::from_slice(&speed_document(entity_id)).expect("the fixture is an entity state"))
    }

    /// The same Vehicle as a Temporal Evolution fragment.
    fn speed_instances(entity_id: &str) -> Arc<EntityDocument> {
        Arc::new(EntityDocument::from_slice(&speed_document(entity_id)).expect("the fixture is an entity document"))
    }

    fn observation_write(entity_id: &str) -> AtomicWrite {
        let uri = entity_id.parse::<Identifier>().expect("valid entity URI");
        AtomicWrite::builder()
            .entity_id(Arc::new(uri))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(speed(entity_id))])
            .build()
    }

    fn evolution_write(entity_id: &str) -> AtomicWrite {
        let uri = entity_id.parse::<Identifier>().expect("valid entity URI");
        AtomicWrite::builder()
            .entity_id(Arc::new(uri))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::EvolutionInstances(speed_instances(entity_id))])
            .build()
    }

    /// A Core write that removed the `speed` Attribute and cleared the Scope.
    fn removal_write(entity_id: &str) -> AtomicWrite {
        let uri = entity_id.parse::<Identifier>().expect("valid entity URI");
        let speed = "https://example.com/speed".parse::<AttributeName>().expect("valid attribute name");
        AtomicWrite::builder()
            .entity_id(Arc::new(uri))
            .entity(EntityMutation::None)
            .history(vec![tombstone(&speed, None, AttributeType::Property), HistoryRecord::ScopeRemoval])
            .build()
    }

    fn record() -> WriteRecord {
        WriteRecord::restored(WriteId::mint(), Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant"))
    }

    /// Two entities landed by the same commit must not restart their ordinal
    /// count at zero: the second entity's entries continue the running counter
    /// where the first entity's left off.
    #[test]
    fn interpret_assigns_each_landed_entitys_intent_a_base_that_continues_the_previous_ones_ordinals() {
        let first = observation_write("urn:ngsi-ld:A");
        let second = observation_write("urn:ngsi-ld:B");
        let (intents, next) =
            interpret(&[&first, &second], &record(), WriteOrdinal::at(0), CoreWriteHistory::Recorded).expect("both entities interpret cleanly");

        pretty_assertions::assert_eq!(intents.len(), 2);
        pretty_assertions::assert_eq!(intents[0].base_ordinal, WriteOrdinal::at(0));
        pretty_assertions::assert_eq!(intents[1].base_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
    }

    /// A replay resumes the running counter at the base it was handed rather
    /// than at zero.
    #[test]
    fn interpret_resumes_the_running_counter_at_a_non_zero_base() {
        let write = observation_write("urn:ngsi-ld:A");
        let (intents, next) = interpret(&[&write], &record(), WriteOrdinal::at(5), CoreWriteHistory::Recorded).expect("the entity interprets cleanly");

        pretty_assertions::assert_eq!(intents[0].base_ordinal, WriteOrdinal::at(5));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(6));
    }

    /// Clause 4.3.1 leaves recording Core writes to the broker: with it off, a
    /// Core write records nothing and draws no ordinal.
    #[test]
    fn an_unrecorded_core_write_does_not_reach_history() {
        let core = observation_write("urn:ngsi-ld:A");
        let removal = removal_write("urn:ngsi-ld:B");

        let (intents, next) =
            interpret(&[&core, &removal], &record(), WriteOrdinal::at(0), CoreWriteHistory::NotRecorded).expect("the entities interpret cleanly");

        assert!(intents.is_empty(), "no history intent: {intents:?}");
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(0));
    }

    /// Turning Core-write history off leaves the Temporal API as it was: every
    /// instance it sends is still history, numbered from the base as though the
    /// unrecorded Core write were not in the commit at all.
    #[test]
    fn temporal_api_writes_are_recorded_when_core_writes_are_not() {
        let core = observation_write("urn:ngsi-ld:A");
        let backfill = evolution_write("urn:ngsi-ld:B");
        let newer = evolution_write("urn:ngsi-ld:C");

        let (intents, next) =
            interpret(&[&core, &backfill, &newer], &record(), WriteOrdinal::at(0), CoreWriteHistory::NotRecorded).expect("every write interprets cleanly");

        let recorded: Vec<&str> = intents.iter().map(|intent| intent.entity_id.as_str()).collect();
        pretty_assertions::assert_eq!(recorded, vec!["urn:ngsi-ld:B", "urn:ngsi-ld:C"]);
        pretty_assertions::assert_eq!(intents[0].base_ordinal, WriteOrdinal::at(0));
        pretty_assertions::assert_eq!(intents[1].base_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
    }

    /// With Core-write history on, a removal reaches history: the tombstone
    /// and the end of the Scope.
    #[test]
    fn a_recorded_core_removal_reaches_history() {
        let removal = removal_write("urn:ngsi-ld:A");

        let (intents, next) = interpret(&[&removal], &record(), WriteOrdinal::at(0), CoreWriteHistory::Recorded).expect("the entity interprets cleanly");

        pretty_assertions::assert_eq!(intents.len(), 1);
        pretty_assertions::assert_eq!(intents[0].entries.len(), 2, "{:?}", intents[0].entries);
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
    }
}
