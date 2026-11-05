//! The outcomes of the writes that change no current state, read under their
//! Entities' row locks.
//!
//! Such a write may record history. A commit asks whether an Entity's history
//! is queued, and keys the intents it queues, under the Entity's identity row
//! lock, which orders the Entity's writes in commit order (clauses 4.5.7, 4.5.8
//! and 4.8). A write of the current state holds the lock from its own
//! statement; this one writes no row, so its row is locked before the commit's
//! other entity statements run.
//!
//! Such a write may also state the version of the current state it was
//! computed from, as a Temporal Evolution backfill that leaves current state as
//! it was does. Its history lands only while that version still stands: after
//! a concurrent write, a deletion included, the write is superseded, records
//! nothing, and its author folds it again against what that write left. The
//! lock reads each row as the writer it waited on left it, so no write slips
//! between the check and the commit.

use crate::{attempt_failure::AttemptFailure, commit_refusal::context_store, entity_group::IndexedWrite};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_version::EntityVersion, write_outcome::WriteOutcome};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_context_store::entity::{locked_entity::LockedEntity, repo::EntityRepository};
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// Lock in `tx` the identity row of every write of `untouched` that records
/// history or states a precondition, and set the outcome of every write of
/// `untouched` in `outcomes`.
///
/// # Errors
///
/// Returns [`AttemptFailure`] when the lock statement fails.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    untouched: &[IndexedWrite<'_>],
    outcomes: &mut [Option<WriteOutcome>],
) -> Result<(), AttemptFailure> {
    let locked = EntityRepository::lock_untouched_in_tx(tx, target, &to_lock(untouched))
        .await
        .map_err(|error| context_store(error))?;
    for (index, outcome) in untouched_outcomes(untouched, locked) {
        outcomes[index] = Some(outcome);
    }
    Ok(())
}

/// Whether `write` takes its Entity's row lock: it records history, which the
/// lock orders, or it states what it assumed of the Entity, which the lock
/// reads.
const fn takes_lock(write: &AtomicWrite) -> bool {
    !write.history.is_empty() || !write.precondition.is_none()
}

/// The Entities of the writes of `untouched` that take their row lock.
fn to_lock<'writes>(untouched: &[IndexedWrite<'writes>]) -> Vec<&'writes Identifier> {
    untouched
        .iter()
        .filter(|(_, write)| takes_lock(write))
        .map(|(_, write)| write.entity_id.as_ref())
        .collect()
}

/// The submission-order position and outcome of each write of `untouched`,
/// held to its precondition against the rows `locked` answered: an Entity no
/// locked row holds has no current state.
fn untouched_outcomes<'writes>(
    untouched: &'writes [IndexedWrite<'writes>],
    locked: Vec<LockedEntity>,
) -> impl Iterator<Item = (usize, WriteOutcome)> + 'writes {
    let current: HashMap<Identifier, Option<EntityVersion>> = locked.into_iter().map(|LockedEntity { id, current }| (id, current)).collect();
    untouched.iter().map(move |(index, write)| {
        let standing = current.get(write.entity_id.as_ref()).copied().flatten();
        (*index, write.precondition.untouched_outcome(standing))
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::untouched_writes::{to_lock, untouched_outcomes};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_version::EntityVersion,
        history_record::HistoryRecord,
        precondition::Precondition,
        write_outcome::WriteOutcome,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_context_store::entity::locked_entity::LockedEntity;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("a valid entity URI")
    }

    fn write(raw: &str) -> AtomicWrite {
        AtomicWrite::builder().entity_id(Arc::new(id(raw))).entity(EntityMutation::None).build()
    }

    /// A backfill computed against `read`, recording one history entry.
    fn backfill(raw: &str, read: EntityVersion) -> AtomicWrite {
        let mut write = AtomicWrite::builder()
            .entity_id(Arc::new(id(raw)))
            .entity(EntityMutation::None)
            .precondition(Precondition::UnchangedSince(read))
            .build();
        write.history = vec![HistoryRecord::ScopeRemoval];
        write
    }

    #[test]
    fn a_write_recording_nothing_and_assuming_nothing_takes_no_lock() {
        let writes = [write("urn:ngsi-ld:A")];
        let untouched: Vec<_> = writes.iter().enumerate().collect();
        assert_eq!(to_lock(&untouched), Vec::<&Identifier>::new());
    }

    #[test]
    fn a_write_recording_history_or_stating_a_precondition_takes_the_lock() {
        let mut recording = write("urn:ngsi-ld:B");
        recording.history = vec![HistoryRecord::ScopeRemoval];
        let assuming = AtomicWrite::builder()
            .entity_id(Arc::new(id("urn:ngsi-ld:C")))
            .entity(EntityMutation::None)
            .precondition(Precondition::MustExist)
            .build();
        let writes = [write("urn:ngsi-ld:A"), recording, assuming];
        let untouched: Vec<_> = writes.iter().enumerate().collect();
        assert_eq!(to_lock(&untouched), vec![&id("urn:ngsi-ld:B"), &id("urn:ngsi-ld:C")]);
    }

    /// A backfill lands while the version it was folded against stands, and is
    /// superseded once a concurrent write moved the row or a deletion took its
    /// current state, so its history is folded again.
    #[test]
    fn a_backfill_lands_only_while_the_version_it_read_stands() {
        let read = EntityVersion::new(4, 2);
        let writes = [
            backfill("urn:ngsi-ld:A", read),
            backfill("urn:ngsi-ld:B", read),
            backfill("urn:ngsi-ld:C", read),
            backfill("urn:ngsi-ld:D", read),
        ];
        let untouched: Vec<_> = writes.iter().enumerate().collect();
        let locked = vec![
            LockedEntity {
                id: id("urn:ngsi-ld:A"),
                current: Some(read),
            },
            LockedEntity {
                id: id("urn:ngsi-ld:B"),
                current: Some(EntityVersion::new(4, 3)),
            },
            LockedEntity {
                id: id("urn:ngsi-ld:C"),
                current: None,
            },
        ];
        let outcomes: Vec<(usize, WriteOutcome)> = untouched_outcomes(&untouched, locked).collect();
        let positions: Vec<usize> = outcomes.iter().map(|(index, _)| *index).collect();
        assert_eq!(positions, vec![0, 1, 2, 3]);
        assert!(outcomes[0].1.is_no_entity_change(), "{:?}", outcomes[0].1);
        for (index, outcome) in &outcomes[1..] {
            assert!(outcome.is_superseded(), "write {index}: {outcome:?}");
        }
    }
}
