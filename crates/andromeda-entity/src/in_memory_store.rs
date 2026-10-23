//! An in-memory entity store that applies each write's precondition the way
//! the PostgreSQL committer does, and lets a test play a concurrent writer.
//!
//! A write that states `UnchangedSince` lands only while the entity still
//! carries that version, and every landed write gives the entity a new one, so
//! a test that changes an entity between an operation's read and its commit
//! sees the operation lose its compare-and-set exactly as it would against
//! another broker replica.

use crate::{
    deletion_outline::{DeletionOutline, DeletionWatchers},
    deletion_outline_fixture::outline_of,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{deletion_outline::DeletionOutlineReader, reader::EntityReader, retrieved_entity::RetrievedEntity},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    precondition::Precondition,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// The stored entities and the clock their versions are drawn from.
#[derive(Default)]
pub(crate) struct StoredRows {
    /// Each entity's state and clause 4.8 `modifiedAt`.
    entities: HashMap<Identifier, (Arc<EntityState>, DateTime<Utc>)>,
    /// The last version handed out.
    clock: DateTime<Utc>,
}

impl StoredRows {
    /// A version later than every one handed out before, so two writes in the
    /// same clock tick still tell apart.
    fn next_version(&mut self) -> DateTime<Utc> {
        self.clock = Utc::now().max(self.clock + TimeDelta::microseconds(1));
        self.clock
    }

    /// Store `state` as the entity `id` names, at a new version.
    pub(crate) fn put(&mut self, id: &Identifier, state: Arc<EntityState>) {
        let version = self.next_version();
        self.entities.insert(id.clone(), (state, version)); // clone: the rows own their keys
    }

    /// Remove the entity `id` names.
    pub(crate) fn remove(&mut self, id: &Identifier) {
        self.entities.remove(id);
    }

    /// The state the entity `id` names, as stored.
    pub(crate) fn payload(&self, id: &Identifier) -> Option<&Arc<EntityState>> {
        self.entities.get(id).map(|(payload, _)| payload)
    }

    /// The version the entity `id` names is stored at.
    pub(crate) fn version(&self, id: &Identifier) -> Option<DateTime<Utc>> {
        self.entities.get(id).map(|(_, modified_at)| *modified_at)
    }

    /// Apply one write the way the store's atomic commit does.
    fn apply(&mut self, write: &AtomicWrite) -> WriteOutcome {
        let id = &*write.entity_id;
        let current = self.entities.get(id).map(|(_, modified_at)| *modified_at);
        match (&write.entity, current, write.precondition) {
            (EntityMutation::Insert(_), Some(_), Precondition::None) => WriteOutcome::Skipped,
            (EntityMutation::Insert(_), Some(_), Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
                WriteOutcome::AlreadyExists
            }
            (EntityMutation::Insert(record), None, _) => {
                self.put(id, Arc::clone(&record.state)); // clone: Arc ref-count bump; the rows share the written state
                WriteOutcome::Created
            }
            // An update or a deletion stating a version matches the row only
            // while it carries that version, and a missing row carries none.
            (EntityMutation::Update(_) | EntityMutation::Delete, None, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
            (EntityMutation::Update(_) | EntityMutation::Delete, Some(modified_at), Precondition::UnchangedSince(expected)) if expected != modified_at => {
                WriteOutcome::Superseded
            }
            (EntityMutation::Update(_) | EntityMutation::Delete, None, Precondition::None | Precondition::MustExist | Precondition::MustNotExist) => {
                WriteOutcome::NotFound
            }
            (EntityMutation::Update(record), Some(_), _) => {
                self.put(id, Arc::clone(&record.state)); // clone: Arc ref-count bump; the rows share the written state
                WriteOutcome::Updated
            }
            (EntityMutation::Delete, Some(modified_at), _) => {
                self.remove(id);
                WriteOutcome::Deleted(DeletedEntity {
                    id: id.clone(), // clone: the outcome owns the identifier
                    created_at: modified_at,
                    modified_at,
                    expires_at: None,
                })
            }
            (EntityMutation::None, _, _) => WriteOutcome::NoEntityChange,
        }
    }
}

/// What another writer does to the store while an operation is under way.
pub(crate) enum ConcurrentWriter {
    /// Changes the rows once, just before the next commit applies.
    BeforeNextCommit(Box<dyn FnOnce(&mut StoredRows) + Send>),
    /// Changes the rows before every commit applies.
    BeforeEveryCommit(Box<dyn Fn(&mut StoredRows) + Send>),
}

/// An in-memory store holding each entity's state and version.
#[derive(Default)]
pub(crate) struct InMemoryStore {
    /// The stored entities.
    rows: Mutex<StoredRows>,
    /// The writer that interferes with the next commits, if any.
    concurrent: Mutex<Option<ConcurrentWriter>>,
    /// Every write submitted, in submission order, whether it landed or not.
    submitted: Mutex<Vec<AtomicWrite>>,
    /// How many reads handed out whole states.
    payload_reads: AtomicUsize,
}

impl InMemoryStore {
    /// Store `state` as the entity `id` names.
    pub(crate) fn seed(&self, id: &Identifier, state: EntityState) {
        self.rows.lock().expect("the store lock is not poisoned").put(id, Arc::new(state));
    }

    /// Have `writer` interfere with the commits that follow.
    pub(crate) fn interfere(&self, writer: ConcurrentWriter) {
        *self.concurrent.lock().expect("the store lock is not poisoned") = Some(writer);
    }

    /// The stored state and version of the entity `id` names.
    pub(crate) fn stored(&self, id: &Identifier) -> Option<(Arc<EntityState>, DateTime<Utc>)> {
        self.rows.lock().expect("the store lock is not poisoned").entities.get(id).cloned() // clone: Arc ref-count bump and the version
    }

    /// Every write submitted for the entity `id` names, in submission order.
    pub(crate) fn submitted_for(&self, id: &Identifier) -> Vec<AtomicWrite> {
        let mut submitted = self.submitted.lock().expect("the store lock is not poisoned");
        let (for_id, others): (Vec<AtomicWrite>, Vec<AtomicWrite>) = submitted.drain(..).partition(|write| *write.entity_id == *id);
        *submitted = others;
        for_id
    }

    /// How many reads handed out whole states.
    pub(crate) fn payload_reads(&self) -> usize {
        self.payload_reads.load(Ordering::Relaxed)
    }

    /// The stored state and version of the entity `id` names, counted as a
    /// state read.
    fn read(&self, id: &Identifier) -> Option<(Arc<EntityState>, DateTime<Utc>)> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        self.stored(id)
    }

    /// Let the concurrent writer, if any, change the rows before a commit.
    fn interfere_before_commit(&self, rows: &mut StoredRows) {
        let mut concurrent = self.concurrent.lock().expect("the store lock is not poisoned");
        match concurrent.take() {
            Some(ConcurrentWriter::BeforeNextCommit(write)) => write(rows),
            Some(ConcurrentWriter::BeforeEveryCommit(write)) => {
                write(rows);
                *concurrent = Some(ConcurrentWriter::BeforeEveryCommit(write));
            }
            None => (),
        }
    }
}

impl ContextCommitter for InMemoryStore {
    fn commit(&self, _target: &ContextTarget, _accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let mut rows = self.rows.lock().expect("the store lock is not poisoned");
        self.interfere_before_commit(&mut rows);
        let outcomes = writes.iter().map(|write| rows.apply(write)).collect();
        drop(rows);
        self.submitted.lock().expect("the store lock is not poisoned").extend(writes);
        Box::pin(async move { Ok(outcomes) })
    }

    fn commit_enqueued(&self, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.commit(&ContextTarget::Live, accepted_at, writes)
    }

    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.commit(target, SystemInstant::new(replay.record.recorded_at()), writes)
    }
}

#[async_trait]
impl DeletionOutlineReader for InMemoryStore {
    async fn deletion_outline(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<DeletionOutline>, EntityError> {
        Ok(self.stored(id).map(|(state, modified_at)| outline_of(&state, modified_at)))
    }

    async fn deletion_watchers(&self, _target: &ContextTarget) -> Result<DeletionWatchers, EntityError> {
        Ok(DeletionWatchers::None)
    }
}

#[async_trait]
impl EntityReader for InMemoryStore {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Ok(self.read(id).map(|(state, _)| state))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.read(id).map(|(state, modified_at)| {
                    let retrieved = RetrievedEntity {
                        state,
                        sys_attrs: SysAttrsSnapshot {
                            created_at: modified_at,
                            modified_at,
                            expires_at: None,
                        },
                    };
                    (id.clone(), retrieved) // clone: the answer owns the identifiers it is keyed by
                })
            })
            .collect())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(self.read(id).map(|(state, modified_at)| EntitySnapshot { state, modified_at }))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.read(id).map(|(state, modified_at)| {
                    (
                        id.clone(), // clone: the answer owns the identifiers it is keyed by
                        EntitySnapshot { state, modified_at },
                    )
                })
            })
            .collect())
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        Ok(self.read(id).map(|(state, modified_at)| RetrievedEntity {
            state,
            sys_attrs: SysAttrsSnapshot {
                created_at: modified_at,
                modified_at,
                expires_at: None,
            },
        }))
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(self.stored(id).map(|(_, modified_at)| SysAttrsSnapshot {
            created_at: modified_at,
            modified_at,
            expires_at: None,
        }))
    }

    async fn get_sys_attrs_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        let mut sys_attrs = HashMap::with_capacity(ids.len());
        for id in ids {
            if let Some(attrs) = self.get_sys_attrs(target, id).await? {
                sys_attrs.insert(id.clone(), attrs); // clone: the answer owns the identifiers it is keyed by
            }
        }
        Ok(sys_attrs)
    }
}
