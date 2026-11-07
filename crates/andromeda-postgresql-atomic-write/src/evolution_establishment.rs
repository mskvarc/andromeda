//! Which Temporal Evolutions a commit's creates establish beside the current
//! state they create, and confirming them against the history the commit
//! records in its own transaction.
//!
//! Clause 4.5.6 brings an Entity's Temporal Evolution into being with the
//! first history that states its types, and a create recorded under
//! [`CoreWriteHistory::Recorded`] states them. When the commit records that
//! history inline, the create mints the identity row with the evolution
//! already in it and its type memberships on both sides, and the history
//! application is told so: it counts the evolution as one it created and
//! writes the row and the memberships no second time. Writing them twice
//! leaves a dead version of each in the heap and in every index on them.
//!
//! The create has to decide before the commit knows where its history lands.
//! An Entity queued behind earlier history, or whose write interprets to no
//! typed intent, does not record inline, and the evolution the create wrote
//! for it is withdrawn ([`confirm`]) before the history is placed.

use crate::{
    attempt_failure::AttemptFailure,
    commit_refusal::context_store,
    history_interpretation::{HistoryRecording, history_recording},
    history_mode::InlineRecording,
};
use andromeda_configuration::schema::temporal::core_write_history::CoreWriteHistory;
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_context_store::entity::repo::EntityRepository;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;

/// The created Entities of `writes` whose create establishes the Temporal
/// Evolution: every insert whose history is interpreted, when the commit may
/// record its history inline.
pub(crate) fn establishing(inline: InlineRecording, writes: &[AtomicWrite], core_write_history: CoreWriteHistory) -> HashSet<Identifier> {
    match inline {
        InlineRecording::Never => HashSet::new(),
        InlineRecording::Possible => writes
            .iter()
            .filter(|write| match write.entity {
                EntityMutation::Insert(_) => history_recording(write, core_write_history) == HistoryRecording::Interpreted,
                EntityMutation::Update(_) | EntityMutation::Delete | EntityMutation::None => false,
            })
            .map(|write| Identifier::clone(&write.entity_id)) // clone: the set outlives the writes it was read from.
            .collect(),
    }
}

/// The Entities of `established` whose Temporal Evolution the commit
/// established, split by whether an intent of `inline` establishes it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Confirmation {
    /// Those an inline intent stating types establishes.
    confirmed: Vec<Identifier>,
    /// The others, whose evolution comes from wherever their history lands.
    withdrawn: Vec<Identifier>,
}

/// Split `established` by whether an intent of `inline` states types for the
/// Entity: only such an intent creates a Temporal Evolution.
fn confirmation(inline: &[TemporalAppendIntent], established: Vec<Identifier>) -> Confirmation {
    let typed: HashSet<&Identifier> = inline
        .iter()
        .filter(|intent| !intent.entity_types.is_empty())
        .map(|intent| &intent.entity_id)
        .collect();
    let (confirmed, withdrawn) = established.into_iter().partition(|id| typed.contains(id));
    Confirmation { confirmed, withdrawn }
}

/// The Entities of `established` whose evolution an intent of `inline`
/// establishes, after withdrawing the evolution of every other one in `tx`.
///
/// # Errors
///
/// Returns [`AttemptFailure`] when the withdrawal fails.
pub(crate) async fn confirm(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    inline: &[TemporalAppendIntent],
    established: Vec<Identifier>,
) -> Result<Vec<Identifier>, AttemptFailure> {
    let Confirmation { confirmed, withdrawn } = confirmation(inline, established);
    EntityRepository::withdraw_established_evolutions_in_tx(tx, target, &withdrawn)
        .await
        .map_err(|error| context_store(error))?;
    Ok(confirmed)
}

#[cfg(test)]
mod tests {
    use crate::{
        evolution_establishment::{Confirmation, confirmation, establishing},
        history_mode::InlineRecording,
    };
    use andromeda_configuration::schema::temporal::core_write_history::CoreWriteHistory;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_record::EntityRecord,
        history_record::HistoryRecord,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_temporal::append_intent::TemporalAppendIntent;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{collections::HashSet, sync::Arc};

    fn vehicle(index: u8) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("a valid identifier")
    }

    fn document(index: u8) -> Vec<u8> {
        serde_json::to_vec(&json!([{
            "@id": format!("urn:ngsi-ld:Vehicle:{index}"),
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            }]
        }]))
        .expect("the fixture serialises")
    }

    fn state(index: u8) -> Arc<EntityState> {
        Arc::new(EntityState::from_slice(&document(index)).expect("the fixture is an entity state"))
    }

    /// A Core create of `vehicle(index)` recording the state it created.
    fn create(index: u8) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(vehicle(index)))
            .entity(EntityMutation::Insert(EntityRecord::from_state(state(index)).expect("a projection")))
            .history(vec![HistoryRecord::Observations(state(index))])
            .build()
    }

    /// A Temporal API create of `vehicle(index)`, recording the instances the
    /// client sent whatever Core writes record.
    fn temporal_create(index: u8) -> AtomicWrite {
        let instances = Arc::new(EntityDocument::from_slice(&document(index)).expect("the fixture is an entity document"));
        AtomicWrite::builder()
            .entity_id(Arc::new(vehicle(index)))
            .entity(EntityMutation::Insert(EntityRecord::from_state(state(index)).expect("a projection")))
            .history(vec![HistoryRecord::EvolutionInstances(instances)])
            .build()
    }

    fn update(index: u8) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(vehicle(index)))
            .entity(EntityMutation::Update(EntityRecord::from_state(state(index)).expect("a projection")))
            .history(vec![HistoryRecord::Observations(state(index))])
            .build()
    }

    fn intent(index: u8, types: Vec<EntityTypeName>) -> TemporalAppendIntent {
        TemporalAppendIntent {
            record: WriteRecord::mint(Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")),
            base_ordinal: WriteOrdinal::at(0),
            entity_id: vehicle(index),
            entity_types: types,
            entries: Vec::new(),
        }
    }

    fn vehicle_type() -> Vec<EntityTypeName> {
        vec!["https://example.com/Vehicle".parse().expect("a valid type name")]
    }

    #[test]
    fn a_commit_that_may_record_inline_establishes_the_evolution_of_its_creates_alone() {
        let writes = [create(1), update(2), create(3)];
        assert_eq!(
            establishing(InlineRecording::Possible, &writes, CoreWriteHistory::Recorded),
            HashSet::from([vehicle(1), vehicle(3)])
        );
    }

    /// A respond-async commit queues its history, so the evolution comes from
    /// the drain.
    #[test]
    fn a_commit_that_never_records_inline_establishes_nothing() {
        let writes = [create(1)];
        assert_eq!(establishing(InlineRecording::Never, &writes, CoreWriteHistory::Recorded), HashSet::new());
    }

    /// A Core create records nothing when Core writes are not recorded, but a
    /// Temporal API create always records the instances it carries.
    #[test]
    fn unrecorded_core_writes_establish_only_the_temporal_creates() {
        let writes = [create(1), temporal_create(2)];
        assert_eq!(
            establishing(InlineRecording::Possible, &writes, CoreWriteHistory::NotRecorded),
            HashSet::from([vehicle(2)])
        );
    }

    #[test]
    fn an_evolution_stands_only_where_an_inline_intent_states_types() {
        let inline = [intent(1, vehicle_type()), intent(2, Vec::new())];
        assert_eq!(
            confirmation(&inline, vec![vehicle(1), vehicle(2), vehicle(3)]),
            Confirmation {
                confirmed: vec![vehicle(1)],
                withdrawn: vec![vehicle(2), vehicle(3)],
            }
        );
    }

    /// An Entity queued behind earlier history has no inline intent at all.
    #[test]
    fn no_inline_intent_withdraws_every_established_evolution() {
        assert_eq!(
            confirmation(&[], vec![vehicle(1)]),
            Confirmation {
                confirmed: Vec::new(),
                withdrawn: vec![vehicle(1)],
            }
        );
    }
}
