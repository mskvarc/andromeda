//! The one interpretation of a commit's temporal consequences.
//!
//! Every landed write's history is interpreted into a temporal append intent
//! exactly once, and the history phase and the current-state projection both
//! read that one interpretation, so they can never disagree about what the
//! write said. A write whose current-state change is not its history -- a
//! Temporal API write, whose history is every instance the client sent while
//! current state takes only the most current one -- states that change itself,
//! and the projection reads the intent interpreted from that instead. A Core
//! write under [`CoreWriteHistory::NotRecorded`] is the other way round: its
//! history is its current-state change, but none of it is written to history.

use crate::commit_refusal::temporal;
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    current_state_projection::CurrentStateProjection,
    error::CommitError,
    history_record::HistoryRecord,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_temporal::{append_intent::TemporalAppendIntent, core_write_history::CoreWriteHistory};
use std::{iter::once, sync::Arc};

/// Interpret every landed write's temporal consequences exactly once, and
/// report the ordinal one past the last one this phase consumed.
///
/// History and the current-attribute projection are two readings of the same
/// mutation, so they read one interpretation rather than each making their
/// own. Every entity a commit lands shares the commit's one write identity, so
/// their intents cannot each restart their own ordinal count at zero: every
/// intent draws its entries' ordinals from the same running counter, seeded at
/// `base`, and the counter advances past exactly the entries that intent used.
///
/// Under [`CoreWriteHistory::NotRecorded`] a Core write -- one whose history
/// is its current-state change -- is interpreted for the projection alone, so
/// Core queries still see it while its Temporal Evolution stays as it was.
pub(crate) fn interpret(
    landed: &[&AtomicWrite],
    record: &WriteRecord,
    base: WriteOrdinal,
    core_write_history: CoreWriteHistory,
) -> Result<(InterpretedHistory, WriteOrdinal), CommitError> {
    let mut next = base;
    let mut interpreted = InterpretedHistory {
        intents: Vec::with_capacity(landed.len()),
        projected: Vec::with_capacity(landed.len()),
    };
    for write in landed {
        let entity_types = write.recorded_entity_types()?;
        match (&write.projection, core_write_history) {
            (CurrentStateProjection::FromHistory, CoreWriteHistory::Recorded) | (CurrentStateProjection::Untouched, _) => {}
            (CurrentStateProjection::FromHistory, CoreWriteHistory::NotRecorded) => {
                // Interpreted for the projection alone: it draws no ordinal,
                // because none of its entries is written to history.
                if let Some(unrecorded) =
                    TemporalAppendIntent::from_history(&write.entity_id, &entity_types, &write.history, *record, next).map_err(temporal)?
                {
                    interpreted.projected.push(ProjectedIntent::Unrecorded(unrecorded));
                }
                continue;
            }
            (CurrentStateProjection::Restated { observed, removed }, _) => {
                let restatement: Vec<HistoryRecord> = once(HistoryRecord::Observations(Arc::clone(observed))) // clone: Arc ref-count bump
                    .chain(removed.iter().cloned().map(HistoryRecord::Tombstone)) // clone: the restatement owns the removals it projects
                    .collect();
                // Interpreted for the projection alone, like an unrecorded
                // Core write.
                if let Some(restated) = TemporalAppendIntent::from_history(&write.entity_id, &entity_types, &restatement, *record, next).map_err(temporal)? {
                    interpreted.projected.push(ProjectedIntent::Unrecorded(restated));
                }
            }
        }
        let Some(intent) = TemporalAppendIntent::from_history(&write.entity_id, &entity_types, &write.history, *record, next).map_err(temporal)? else {
            continue;
        };
        let advance = u32::try_from(intent.entries.len()).map_err(|error| CommitError::Invariant {
            detail: format!(
                "temporal intent for entity {} carries more entries than an ordinal can number: {error}",
                write.entity_id
            ),
        })?;
        next = next.advanced_by(advance).map_err(|error| CommitError::Invariant {
            detail: format!("temporal consequence ordinal overflowed past entity {}: {error}", write.entity_id),
        })?;
        if matches!(write.projection, CurrentStateProjection::FromHistory) {
            interpreted.projected.push(ProjectedIntent::History(interpreted.intents.len()));
        }
        interpreted.intents.push(intent);
    }
    Ok((interpreted, next))
}

/// One intent the current-state projection follows.
enum ProjectedIntent {
    /// The history intent at this position.
    History(usize),
    /// An intent none of whose entries is written to history: a Temporal API
    /// write's restatement of current state, or a Core write under
    /// [`CoreWriteHistory::NotRecorded`].
    Unrecorded(TemporalAppendIntent),
}

/// Every landed write's interpreted history, and what each says of current
/// state.
pub(crate) struct InterpretedHistory {
    /// One intent per landed write that carried history, in write order.
    pub(crate) intents: Vec<TemporalAppendIntent>,
    /// What the current-state projection follows, in write order.
    projected: Vec<ProjectedIntent>,
}

impl InterpretedHistory {
    /// The intents the current-state projection follows.
    ///
    /// `current_attribute_values` stays the projection of the Core document it
    /// answers `q` for: a Core write's history is its current-state change,
    /// recorded or not, and a Temporal API write projects only the instances
    /// that stood, never every instance it recorded.
    pub(crate) fn projected(&self) -> Vec<&TemporalAppendIntent> {
        self.projected
            .iter()
            .filter_map(|projected| match projected {
                ProjectedIntent::History(position) => self.intents.get(*position),
                ProjectedIntent::Unrecorded(intent) => Some(intent),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::history_interpretation::interpret;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        current_state_projection::CurrentStateProjection,
        entity_mutation::EntityMutation,
        history_record::{HistoryRecord, tombstone},
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
    use andromeda_temporal::{append_intent::TemporalAppendEntry, core_write_history::CoreWriteHistory};
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn speed(entity_id: &str) -> Arc<Value> {
        Arc::new(json!([{
            "@id": entity_id,
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            }]
        }]))
    }

    fn observation_write(entity_id: &str) -> AtomicWrite {
        let uri = entity_id.parse::<Identifier>().expect("valid entity URI");
        AtomicWrite::builder()
            .entity_id(Arc::new(uri))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(speed(entity_id))])
            .build()
    }

    fn evolution_write(entity_id: &str, projection: CurrentStateProjection) -> AtomicWrite {
        let uri = entity_id.parse::<Identifier>().expect("valid entity URI");
        AtomicWrite::builder()
            .entity_id(Arc::new(uri))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::EvolutionInstances(speed(entity_id))])
            .projection(projection)
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
        let (interpreted, next) =
            interpret(&[&first, &second], &record(), WriteOrdinal::at(0), CoreWriteHistory::Recorded).expect("both entities interpret cleanly");

        pretty_assertions::assert_eq!(interpreted.intents.len(), 2);
        pretty_assertions::assert_eq!(interpreted.intents[0].base_ordinal, WriteOrdinal::at(0));
        pretty_assertions::assert_eq!(interpreted.intents[1].base_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
    }

    /// A replay resumes the running counter at the base it was handed rather
    /// than at zero.
    #[test]
    fn interpret_resumes_the_running_counter_at_a_non_zero_base() {
        let write = observation_write("urn:ngsi-ld:A");
        let (interpreted, next) = interpret(&[&write], &record(), WriteOrdinal::at(5), CoreWriteHistory::Recorded).expect("the entity interprets cleanly");

        pretty_assertions::assert_eq!(interpreted.intents[0].base_ordinal, WriteOrdinal::at(5));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(6));
    }

    /// A Temporal API write's instances all reach history, while the projection
    /// follows what the write says stood -- nothing when current state was left
    /// as it was.
    #[test]
    fn a_temporal_api_write_projects_its_restatement_and_not_its_history() {
        let core = observation_write("urn:ngsi-ld:A");
        let backfill = evolution_write("urn:ngsi-ld:B", CurrentStateProjection::Untouched);
        let newer = evolution_write(
            "urn:ngsi-ld:C",
            CurrentStateProjection::Restated {
                observed: speed("urn:ngsi-ld:C"),
                removed: Vec::new(),
            },
        );
        let (interpreted, next) =
            interpret(&[&core, &backfill, &newer], &record(), WriteOrdinal::at(0), CoreWriteHistory::Recorded).expect("every write interprets cleanly");

        pretty_assertions::assert_eq!(interpreted.intents.len(), 3, "every write reaches history");
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(3), "the restatement draws no ordinal");
        let projected: Vec<&str> = interpreted.projected().iter().map(|intent| intent.entity_id.as_str()).collect();
        pretty_assertions::assert_eq!(projected, vec!["urn:ngsi-ld:A", "urn:ngsi-ld:C"]);
    }

    /// Clause 4.3.1 leaves recording Core writes to the broker: with it off, a
    /// Core write reaches the current-state projection and not history, and
    /// draws no ordinal for entries that are never written.
    #[test]
    fn an_unrecorded_core_write_is_projected_without_reaching_history() {
        let core = observation_write("urn:ngsi-ld:A");

        let (interpreted, next) = interpret(&[&core], &record(), WriteOrdinal::at(0), CoreWriteHistory::NotRecorded).expect("the entity interprets cleanly");

        assert!(interpreted.intents.is_empty(), "no history intent: {:?}", interpreted.intents);
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(0));
        let projected = interpreted.projected();
        pretty_assertions::assert_eq!(projected.len(), 1);
        pretty_assertions::assert_eq!(projected[0].entity_id.as_str(), "urn:ngsi-ld:A");
        assert!(
            projected[0]
                .entries
                .iter()
                .any(|entry| matches!(entry, TemporalAppendEntry::Observation { .. })),
            "the projection follows the written instance: {:?}",
            projected[0].entries
        );
    }

    /// The removals a Core write makes still leave current state, through the
    /// projection, while history records neither the tombstone nor the end of
    /// the Scope.
    #[test]
    fn an_unrecorded_core_removal_is_projected_without_reaching_history() {
        let removal = removal_write("urn:ngsi-ld:A");

        let (interpreted, next) = interpret(&[&removal], &record(), WriteOrdinal::at(0), CoreWriteHistory::NotRecorded).expect("the entity interprets cleanly");

        assert!(interpreted.intents.is_empty(), "no history intent: {:?}", interpreted.intents);
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(0));
        let projected = interpreted.projected();
        pretty_assertions::assert_eq!(projected.len(), 1);
        assert!(
            projected[0].entries.iter().any(|entry| matches!(entry, TemporalAppendEntry::Tombstone { .. })),
            "the projection drops the removed instance: {:?}",
            projected[0].entries
        );
    }

    /// Turning Core-write history off leaves the Temporal API as it was: every
    /// instance it sends is still history, numbered from the base as though the
    /// unrecorded Core write were not in the commit at all.
    #[test]
    fn temporal_api_writes_are_recorded_when_core_writes_are_not() {
        let core = observation_write("urn:ngsi-ld:A");
        let backfill = evolution_write("urn:ngsi-ld:B", CurrentStateProjection::Untouched);
        let newer = evolution_write(
            "urn:ngsi-ld:C",
            CurrentStateProjection::Restated {
                observed: speed("urn:ngsi-ld:C"),
                removed: Vec::new(),
            },
        );

        let (interpreted, next) =
            interpret(&[&core, &backfill, &newer], &record(), WriteOrdinal::at(0), CoreWriteHistory::NotRecorded).expect("every write interprets cleanly");

        let recorded: Vec<&str> = interpreted.intents.iter().map(|intent| intent.entity_id.as_str()).collect();
        pretty_assertions::assert_eq!(recorded, vec!["urn:ngsi-ld:B", "urn:ngsi-ld:C"]);
        pretty_assertions::assert_eq!(interpreted.intents[0].base_ordinal, WriteOrdinal::at(0));
        pretty_assertions::assert_eq!(interpreted.intents[1].base_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
        let projected: Vec<&str> = interpreted.projected().iter().map(|intent| intent.entity_id.as_str()).collect();
        pretty_assertions::assert_eq!(projected, vec!["urn:ngsi-ld:A", "urn:ngsi-ld:C"]);
    }

    /// With Core-write history on, the same removal reaches history.
    #[test]
    fn a_recorded_core_removal_reaches_history() {
        let removal = removal_write("urn:ngsi-ld:A");

        let (interpreted, next) = interpret(&[&removal], &record(), WriteOrdinal::at(0), CoreWriteHistory::Recorded).expect("the entity interprets cleanly");

        pretty_assertions::assert_eq!(interpreted.intents.len(), 1);
        pretty_assertions::assert_eq!(interpreted.intents[0].entries.len(), 2, "{:?}", interpreted.intents[0].entries);
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
    }
}
