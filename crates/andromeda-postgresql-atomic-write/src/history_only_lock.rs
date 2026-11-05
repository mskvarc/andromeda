//! The identity row lock of a write that records history and changes no
//! current state.
//!
//! A commit asks whether an Entity's history is queued, and keys the intents it
//! queues, under the Entity's identity row lock, which orders the Entity's
//! writes in commit order (clauses 4.5.7, 4.5.8 and 4.8). A write of the
//! current state holds the lock from its own statement. A write that leaves the
//! current state as it was writes no row, so on the live branch, the one that
//! queues history, its row is locked before anything is placed. A Snapshot
//! branch records every history inline and queues nothing.

use crate::{attempt_failure::AttemptFailure, commit_refusal::context_store};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_context_store::entity::repo::EntityRepository;
use sqlx::{Postgres, Transaction};

/// Lock in `tx` the identity row of every write of `writes` that records
/// history and mutates no current state, when `target` queues history.
///
/// # Errors
///
/// Returns [`AttemptFailure`] when the lock statement fails.
pub(crate) async fn apply(tx: &mut Transaction<'_, Postgres>, target: &ContextTarget, writes: &[AtomicWrite]) -> Result<(), AttemptFailure> {
    match target {
        ContextTarget::Live => {
            let ids = history_only(writes);
            EntityRepository::lock_for_history_in_tx(tx, target, &ids)
                .await
                .map_err(|error| context_store(error))?;
            Ok(())
        }
        ContextTarget::Snapshot(_) => Ok(()),
    }
}

/// The Entities of the writes of `writes` that record history and mutate no
/// current state.
fn history_only(writes: &[AtomicWrite]) -> Vec<&Identifier> {
    writes
        .iter()
        .filter(|write| match write.entity {
            EntityMutation::None => !write.history.is_empty(),
            EntityMutation::Insert(_) | EntityMutation::Update(_) | EntityMutation::Delete => false,
        })
        .map(|write| write.entity_id.as_ref())
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history_only_lock::history_only;
    use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, history_record::HistoryRecord};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn write(id: &str, entity: EntityMutation) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Identifier>().expect("a valid entity URI")))
            .entity(entity)
            .build()
    }

    /// A write that records nothing queues nothing, and one that writes the
    /// current state holds its row's lock from its own statement.
    #[test]
    fn neither_a_write_without_history_nor_a_current_state_write_takes_the_history_lock() {
        let writes = [write("urn:ngsi-ld:A", EntityMutation::None), write("urn:ngsi-ld:B", EntityMutation::Delete)];
        assert_eq!(history_only(&writes), Vec::<&Identifier>::new());
    }

    #[test]
    fn a_write_recording_history_without_a_current_state_change_takes_the_history_lock() {
        let mut backfill = write("urn:ngsi-ld:C", EntityMutation::None);
        backfill.history = vec![HistoryRecord::ScopeRemoval];
        let writes = [write("urn:ngsi-ld:A", EntityMutation::None), backfill];
        let expected: Identifier = "urn:ngsi-ld:C".parse().expect("a valid entity URI");
        assert_eq!(history_only(&writes), vec![&expected]);
    }
}
