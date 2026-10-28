//! An in-memory entity store that applies each write's precondition the way
//! the PostgreSQL committer does, and lets a test play a concurrent writer.
//!
//! A write that states `UnchangedSince` lands only while the entity still
//! carries that version, and every landed write gives the entity a new one, so
//! a test that changes an entity between an operation's read and its commit
//! sees the operation lose its compare-and-set exactly as it would against
//! another broker replica. The version is the row's key and write count, as
//! the PostgreSQL store keeps it: a row is keyed once and counts every write,
//! and a deleted entity created anew is keyed again. A committed write stamps
//! its `modifiedAt` with the instant it was accepted at, so two writes
//! accepted at the same instant leave the same `modifiedAt` and still two
//! versions.

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
    entity_version::EntityVersion,
    precondition::Precondition,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// One stored entity.
#[derive(Clone)]
struct StoredRow {
    /// The entity's state.
    state: Arc<EntityState>,
    /// The version the row stands at.
    version: EntityVersion,
    /// The clause 4.8 `modifiedAt` the last write stamped.
    modified_at: DateTime<Utc>,
}

/// The stored entities and the last row key handed out.
#[derive(Default)]
pub(crate) struct StoredRows {
    /// Each entity's row.
    entities: HashMap<Identifier, StoredRow>,
    /// The key the last new row was given.
    last_key: i64,
}

impl StoredRows {
    /// Store `state` as the entity `id` names, at a new version, modified now.
    pub(crate) fn put(&mut self, id: &Identifier, state: Arc<EntityState>) {
        self.write(id, state, Utc::now());
    }

    /// Store `state` as the entity `id` names, modified at `modified_at`: a
    /// standing row counts one more write, a new one is keyed afresh.
    fn write(&mut self, id: &Identifier, state: Arc<EntityState>, modified_at: DateTime<Utc>) {
        let version = if let Some(row) = self.entities.get(id) {
            EntityVersion::new(row.version.entity_key(), row.version.write_count() + 1)
        } else {
            self.last_key += 1;
            EntityVersion::new(self.last_key, 1)
        };
        // clone: the rows own their keys
        self.entities.insert(id.clone(), StoredRow { state, version, modified_at });
    }

    /// Remove the entity `id` names.
    pub(crate) fn remove(&mut self, id: &Identifier) {
        self.entities.remove(id);
    }

    /// The state the entity `id` names, as stored.
    pub(crate) fn payload(&self, id: &Identifier) -> Option<&Arc<EntityState>> {
        self.entities.get(id).map(|row| &row.state)
    }

    /// The version the entity `id` names is stored at.
    pub(crate) fn version(&self, id: &Identifier) -> Option<EntityVersion> {
        self.entities.get(id).map(|row| row.version)
    }

    /// Apply one write accepted at `accepted_at` the way the store's atomic
    /// commit does.
    fn apply(&mut self, write: &AtomicWrite, accepted_at: DateTime<Utc>) -> WriteOutcome {
        let id = &*write.entity_id;
        let current = self.entities.get(id).map(|row| (row.version, row.modified_at));
        match (&write.entity, current, write.precondition) {
            (EntityMutation::Insert(_), Some(_), Precondition::None) => WriteOutcome::Skipped,
            (EntityMutation::Insert(_), Some(_), Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
                WriteOutcome::AlreadyExists
            }
            (EntityMutation::Insert(record), None, _) => {
                self.write(id, Arc::clone(&record.state), accepted_at); // clone: Arc ref-count bump; the rows share the written state
                WriteOutcome::Created
            }
            // An update or a deletion stating a version matches the row only
            // while it carries that version, and a missing row carries none.
            (EntityMutation::Update(_) | EntityMutation::Delete, None, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
            (EntityMutation::Update(_) | EntityMutation::Delete, Some((version, _)), Precondition::UnchangedSince(expected)) if expected != version => {
                WriteOutcome::Superseded
            }
            (EntityMutation::Update(_) | EntityMutation::Delete, None, Precondition::None | Precondition::MustExist | Precondition::MustNotExist) => {
                WriteOutcome::NotFound
            }
            (EntityMutation::Update(record), Some(_), _) => {
                self.write(id, Arc::clone(&record.state), accepted_at); // clone: Arc ref-count bump; the rows share the written state
                WriteOutcome::Updated
            }
            (EntityMutation::Delete, Some((_, modified_at)), _) => {
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

impl StoredRow {
    /// The snapshot a read-modify-write states its precondition against.
    fn into_snapshot(self) -> EntitySnapshot {
        EntitySnapshot {
            state: self.state,
            version: self.version,
        }
    }
}

/// The system times of an entity last modified at `modified_at`, which the
/// store also answers as its creation.
const fn sys_attrs(modified_at: DateTime<Utc>) -> SysAttrsSnapshot {
    SysAttrsSnapshot {
        created_at: modified_at,
        modified_at,
        expires_at: None,
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
    pub(crate) fn stored(&self, id: &Identifier) -> Option<(Arc<EntityState>, EntityVersion)> {
        self.row(id).map(|row| (row.state, row.version))
    }

    /// The clause 4.8 `modifiedAt` the entity `id` names was last stamped with.
    pub(crate) fn modified_at(&self, id: &Identifier) -> Option<DateTime<Utc>> {
        self.row(id).map(|row| row.modified_at)
    }

    /// The row of the entity `id` names.
    fn row(&self, id: &Identifier) -> Option<StoredRow> {
        self.rows.lock().expect("the store lock is not poisoned").entities.get(id).cloned() // clone: Arc ref-count bump, the version and the instant
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

    /// The row of the entity `id` names, counted as a state read.
    fn read(&self, id: &Identifier) -> Option<StoredRow> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        self.row(id)
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
    fn commit(&self, _target: &ContextTarget, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let mut rows = self.rows.lock().expect("the store lock is not poisoned");
        self.interfere_before_commit(&mut rows);
        let outcomes = writes.iter().map(|write| rows.apply(write, accepted_at.get())).collect();
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
        Ok(self.stored(id).map(|(state, version)| outline_of(&state, version)))
    }

    async fn deletion_watchers(&self, _target: &ContextTarget) -> Result<DeletionWatchers, EntityError> {
        Ok(DeletionWatchers::None)
    }
}

#[async_trait]
impl EntityReader for InMemoryStore {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Ok(self.read(id).map(|row| row.state))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.read(id).map(|row| {
                    let retrieved = RetrievedEntity {
                        state: row.state,
                        sys_attrs: sys_attrs(row.modified_at),
                    };
                    (id.clone(), retrieved) // clone: the answer owns the identifiers it is keyed by
                })
            })
            .collect())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(self.read(id).map(StoredRow::into_snapshot))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.read(id).map(|row| {
                    (
                        id.clone(), // clone: the answer owns the identifiers it is keyed by
                        row.into_snapshot(),
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
        Ok(self.read(id).map(|row| RetrievedEntity {
            state: row.state,
            sys_attrs: sys_attrs(row.modified_at),
        }))
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(self.modified_at(id).map(sys_attrs))
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

#[cfg(test)]
mod tests {
    use crate::{
        compare_and_set_fixture::{attribute_terms, building_id, stored_building},
        in_memory_store::InMemoryStore,
    };
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        committer::ContextCommitter,
        entity_mutation::EntityMutation,
        entity_record::EntityRecord,
        entity_version::EntityVersion,
        precondition::Precondition,
        write_outcome::WriteOutcome,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    /// The instant every write of a test is accepted at.
    fn accepted_at() -> SystemInstant {
        SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 28, 12, 0, 0).single().expect("a valid instant"))
    }

    /// An update of `previous` to the Building `id` holding `attributes`,
    /// stated against `read`.
    fn update(id: &Identifier, previous: &Arc<EntityState>, attributes: &[&str], read: EntityVersion) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns its identifier
            .entity(EntityMutation::Update(
                EntityRecord::replacing(previous, Arc::new(stored_building(id, attributes))).expect("a projectable state"),
            ))
            .precondition(Precondition::UnchangedSince(read))
            .build()
    }

    /// Commit `write` at the shared instant and answer its one outcome.
    async fn commit(store: &InMemoryStore, write: AtomicWrite) -> WriteOutcome {
        let mut outcomes = store.commit(&ContextTarget::Live, accepted_at(), vec![write]).await.expect("the commit runs");
        outcomes.pop().expect("one outcome per write")
    }

    /// Writer `b` read the version `a` left and committed in the same instant
    /// as `a`, so both stamped the same `modifiedAt`. A reader holding `a`'s
    /// version still loses its compare-and-set, and `b`'s change survives.
    #[tokio::test]
    async fn a_write_stating_a_version_replaced_in_the_same_instant_is_superseded() {
        let store = InMemoryStore::default();
        let id = building_id("a");
        store.seed(&id, stored_building(&id, &["seed"]));
        let (seeded, seeded_version) = store.stored(&id).expect("the Building is stored");

        assert!(commit(&store, update(&id, &seeded, &["a"], seeded_version)).await.is_updated());
        let (read_by_a, version_a) = store.stored(&id).expect("the Building is stored");
        let stamped_by_a = store.modified_at(&id);
        assert!(commit(&store, update(&id, &read_by_a, &["b"], version_a)).await.is_updated());
        assert_eq!(store.modified_at(&id), stamped_by_a, "both writes stamped the same instant");

        assert!(commit(&store, update(&id, &read_by_a, &["stale"], version_a)).await.is_superseded());
        let (stored, _) = store.stored(&id).expect("the Building is stored");
        assert_eq!(attribute_terms(&stored), vec!["b"]);
    }

    /// An Entity deleted and created anew starts its write count over, on a
    /// row keyed afresh, so a version read before the deletion never matches
    /// the new Entity.
    #[tokio::test]
    async fn a_write_stating_a_version_of_a_deleted_entity_does_not_land_on_its_successor() {
        let store = InMemoryStore::default();
        let id = building_id("a");
        store.seed(&id, stored_building(&id, &["first"]));
        let (first, first_version) = store.stored(&id).expect("the Building is stored");

        let deletion = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns its identifier
            .entity(EntityMutation::Delete)
            .precondition(Precondition::UnchangedSince(first_version))
            .build();
        assert!(commit(&store, deletion).await.is_deleted());
        store.seed(&id, stored_building(&id, &["second"]));
        let (_, second_version) = store.stored(&id).expect("the Building is stored again");
        assert_eq!(second_version.write_count(), first_version.write_count());

        assert!(commit(&store, update(&id, &first, &["stale"], first_version)).await.is_superseded());
        let (stored, _) = store.stored(&id).expect("the Building is stored");
        assert_eq!(attribute_terms(&stored), vec!["second"]);
    }
}
