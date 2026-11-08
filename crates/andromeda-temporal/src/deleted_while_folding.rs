//! One Entity whose current state a concurrent deletion takes between the read
//! a Temporal Evolution write folds against and the write's commit.
//!
//! Each read answers the next state the Entity is read at, and each commit is
//! judged against the Entity as the deletion left it: no current state. A write
//! that changes no current state is held to its precondition by the rule the
//! store applies, and a creation lands while no current state stands.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    entity_mutation::EntityMutation,
    entity_version::EntityVersion,
    precondition::Precondition,
    store_instant::StoreInstant,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{reader::EntityReader, retrieved_entity::RetrievedEntity},
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use async_trait::async_trait;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

/// What one commit attempted for the Entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// A write leaving current state as it was, stating `Precondition`.
    Untouched(Precondition),
    /// A creation of the Entity, stating `Precondition`.
    Creation(Precondition),
}

/// The Entity, its reads and the commits it was sent.
pub(crate) struct DeletedWhileFolding {
    /// What each read of the Entity answers, in turn.
    reads: Mutex<VecDeque<Option<EntitySnapshot>>>,
    /// The version the Entity's current state stands at, `None` while it has
    /// none.
    current: Mutex<Option<EntityVersion>>,
    /// What each commit attempted, in order.
    attempts: Mutex<Vec<Attempt>>,
}

impl DeletedWhileFolding {
    /// The Entity read first at `read` and then without a current state, which
    /// a deletion committed right after the first read took.
    pub(crate) fn new(read: EntitySnapshot) -> DeletedWhileFolding {
        DeletedWhileFolding {
            reads: Mutex::new(VecDeque::from([Some(read), None])),
            current: Mutex::new(None),
            attempts: Mutex::new(Vec::new()),
        }
    }

    /// What every commit attempted, in the order they ran.
    pub(crate) fn attempts(&self) -> Vec<Attempt> {
        self.attempts.lock().expect("the recording lock is never poisoned").clone()
    }

    /// Judge `write`, accepted at `accepted_at`, against the Entity.
    fn attempt(&self, write: &AtomicWrite, accepted_at: SystemInstant) -> WriteOutcome {
        let mut current = self.current.lock().expect("the state lock is never poisoned");
        let mut attempts = self.attempts.lock().expect("the recording lock is never poisoned");
        match &write.entity {
            EntityMutation::None => {
                attempts.push(Attempt::Untouched(write.precondition));
                write.precondition.untouched_outcome(*current)
            }
            EntityMutation::Insert(_) => {
                attempts.push(Attempt::Creation(write.precondition));
                if current.is_some() {
                    return WriteOutcome::AlreadyExists;
                }
                *current = Some(EntityVersion::new(2, 1));
                WriteOutcome::Created(StoreInstant::new(accepted_at.get()))
            }
            EntityMutation::Update(_) | EntityMutation::Delete => panic!("no fold of a deleted Entity updates or deletes it"),
        }
    }
}

#[async_trait]
impl EntityReader for DeletedWhileFolding {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        panic!("a Temporal Evolution write reads the Entity's snapshot")
    }

    async fn get_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        panic!("a Temporal Evolution write reads the Entity's snapshot")
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        panic!("a Temporal Evolution write reads the Entity's snapshot")
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(self
            .reads
            .lock()
            .expect("the read lock is never poisoned")
            .pop_front()
            .expect("the Entity is read no more often than scripted"))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        panic!("a Temporal Evolution write reads one Entity's snapshot")
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        panic!("a Temporal Evolution write reads the Entity's snapshot")
    }
}

impl ContextCommitter for DeletedWhileFolding {
    fn commit(&self, _target: &ContextTarget, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let outcomes = writes.iter().map(|write| self.attempt(write, accepted_at)).collect();
        Box::pin(async move { Ok(outcomes) })
    }

    fn commit_enqueued(&self, _accepted_at: SystemInstant, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("a Temporal Evolution write commits synchronously") })
    }

    fn commit_replayed(&self, _target: &ContextTarget, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("a Temporal Evolution write is never replayed") })
    }
}
