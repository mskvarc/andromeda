//! The lock a commit takes on the identity rows it writes, before any of its
//! entity statements run.
//!
//! A commit's entity statements each reach their rows in submission order,
//! inserts before updates before deletions, while the history drain locks the
//! rows of a batch of Entities in identifier order. Two transactions holding
//! one row each and waiting for the other's deadlock, and PostgreSQL breaks
//! the cycle only after `deadlock_timeout`, rolling one of them back. A commit
//! writing more than one Entity therefore locks every existing row it writes
//! first, in identifier order, so none of its statements takes a row lock out
//! of that order. A commit writing one Entity holds one row lock and waits on
//! no other row, so it takes no lock of its own, unless its write changes no
//! current state and reads its row under the lock.

use crate::{attempt_failure::AttemptFailure, commit_refusal::context_store, untouched_writes};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_context_store::entity::{commit_row_lock::CommitRowLock, locked_entity::LockedEntity, repo::EntityRepository};
use sqlx::{Postgres, Transaction};

/// The rows a commit locks before its writes, and how strongly.
#[derive(Debug, PartialEq, Eq)]
struct LockPlan<'writes> {
    /// The Entities whose rows are locked, each once.
    ids: Vec<&'writes Identifier>,
    /// The lock taken on each.
    lock: CommitRowLock,
}

/// Lock in `tx` the identity rows the commit of `writes` locks before its
/// writes (see [`plan`]), answering each locked row.
///
/// # Errors
///
/// Returns [`AttemptFailure`] when the lock statement fails.
pub(crate) async fn lock(tx: &mut Transaction<'_, Postgres>, target: &ContextTarget, writes: &[AtomicWrite]) -> Result<Vec<LockedEntity>, AttemptFailure> {
    let Some(LockPlan { ids, lock }) = plan(writes) else {
        return Ok(Vec::new());
    };
    let locked = EntityRepository::lock_commit_rows_in_tx(tx, target, &ids, lock)
        .await
        .map_err(|error| context_store(error))?;
    Ok(locked)
}

/// The rows the commit of `writes` locks before its writes: every Entity a
/// write mutates, and every Entity a write that changes no current state reads
/// under the lock. Nothing is locked when that is one Entity a mutation's own
/// statement locks. A commit deleting any Entity takes the lock its deletion
/// needs on every row.
fn plan(writes: &[AtomicWrite]) -> Option<LockPlan<'_>> {
    let mut ids: Vec<&Identifier> = writes.iter().filter(|write| locks_row(write)).map(|write| write.entity_id.as_ref()).collect();
    ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    ids.dedup_by(|left, right| left.as_str() == right.as_str());
    let ids = ids;
    let read_under_lock = writes
        .iter()
        .any(|write| matches!(write.entity, EntityMutation::None) && untouched_writes::reads_its_row(write));
    if ids.is_empty() || (ids.len() == 1 && !read_under_lock) {
        return None;
    }
    let lock = if writes.iter().any(|write| matches!(write.entity, EntityMutation::Delete)) {
        CommitRowLock::Deleting
    } else {
        CommitRowLock::Writing
    };
    Some(LockPlan { ids, lock })
}

/// Whether the commit locks the row of `write`'s Entity before its writes.
const fn locks_row(write: &AtomicWrite) -> bool {
    match write.entity {
        EntityMutation::Insert(_) | EntityMutation::Update(_) | EntityMutation::Delete => true,
        EntityMutation::None => untouched_writes::reads_its_row(write),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::commit_lock::{LockPlan, plan};
    use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, history_record::HistoryRecord};
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_context_store::entity::commit_row_lock::CommitRowLock;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("a valid entity URI")
    }

    fn write(raw: &str, entity: EntityMutation) -> AtomicWrite {
        AtomicWrite::builder().entity_id(Arc::new(id(raw))).entity(entity).build()
    }

    fn update(raw: &str) -> AtomicWrite {
        let record = EntityRecord::from_state(Arc::new(EntityState::new(
            id(raw),
            EntityTypes::one("https://example.org/T".parse().expect("valid type name")),
        )))
        .expect("a projectable state");
        write(raw, EntityMutation::Update(record))
    }

    fn recording(raw: &str) -> AtomicWrite {
        let mut write = write(raw, EntityMutation::None);
        write.history = vec![HistoryRecord::ScopeRemoval];
        write
    }

    /// One mutated Entity holds one row lock, which its own statement takes,
    /// and waits on no other row.
    #[test]
    fn a_commit_writing_one_entity_takes_no_lock_of_its_own() {
        for writes in [
            vec![update("urn:ngsi-ld:A")],
            vec![write("urn:ngsi-ld:A", EntityMutation::Delete)],
            vec![update("urn:ngsi-ld:A"), update("urn:ngsi-ld:A")],
        ] {
            assert_eq!(plan(&writes), None);
        }
    }

    /// A write that leaves current state as it was writes no row, so its lock
    /// is the commit's to take, and it reads the row under it.
    #[test]
    fn a_write_reading_its_row_under_the_lock_is_locked_alone() {
        let writes = [recording("urn:ngsi-ld:A")];
        assert_eq!(
            plan(&writes),
            Some(LockPlan {
                ids: vec![&id("urn:ngsi-ld:A")],
                lock: CommitRowLock::Writing,
            })
        );
    }

    /// The rows are named in the order the lock takes them, each once, and a
    /// write that neither records history nor states a precondition reads
    /// nothing under the lock.
    #[test]
    fn a_commit_writing_several_entities_locks_each_once_in_identifier_order() {
        let writes = [
            update("urn:ngsi-ld:C"),
            write("urn:ngsi-ld:D", EntityMutation::None),
            update("urn:ngsi-ld:A"),
            update("urn:ngsi-ld:C"),
            recording("urn:ngsi-ld:B"),
        ];
        assert_eq!(
            plan(&writes),
            Some(LockPlan {
                ids: vec![&id("urn:ngsi-ld:A"), &id("urn:ngsi-ld:B"), &id("urn:ngsi-ld:C")],
                lock: CommitRowLock::Writing,
            })
        );
    }

    /// A deletion needs `FOR UPDATE`, so a commit holding a weaker lock would
    /// raise it while others wait on the row.
    #[test]
    fn a_commit_deleting_any_entity_locks_every_row_for_its_deletion() {
        let writes = [update("urn:ngsi-ld:A"), write("urn:ngsi-ld:B", EntityMutation::Delete)];
        assert_eq!(plan(&writes).map(|plan| plan.lock), Some(CommitRowLock::Deleting));
    }
}
