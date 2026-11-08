//! An in-memory entity store that applies each write's precondition the way
//! the PostgreSQL committer does, and lets a test play a concurrent writer.
//!
//! A test that changes an entity between an operation's read and its commit
//! sees the operation lose its compare-and-set exactly as it would against
//! another broker replica.

use crate::{
    deletion_outline::{DeletionOutline, DeletionWatchers},
    deletion_outline_fixture::outline_of,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    in_memory_rows::{StoredRow, StoredRows},
    port::{deletion_outline::DeletionOutlineReader, reader::EntityReader, retrieved_entity::RetrievedEntity},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    entity_version::EntityVersion,
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

/// The snapshot a read-modify-write states its precondition against.
fn snapshot_of(row: StoredRow) -> EntitySnapshot {
    EntitySnapshot {
        state: row.state,
        version: row.version,
        sys_attrs: sys_attrs(row.modified_at),
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

/// A write another writer lands just before the next read.
type BeforeNextRead = Box<dyn FnOnce(&mut StoredRows) + Send>;

/// An in-memory store holding each entity's state and version.
#[derive(Default)]
pub(crate) struct InMemoryStore {
    /// The stored entities.
    rows: Mutex<StoredRows>,
    /// The writer that interferes with the next commits, if any.
    concurrent: Mutex<Option<ConcurrentWriter>>,
    /// The writer that lands just before the next read, if any.
    before_read: Mutex<Option<BeforeNextRead>>,
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

    /// Store `state` as the entity `id` names, modified at `modified_at`.
    pub(crate) fn seed_at(&self, id: &Identifier, state: EntityState, modified_at: DateTime<Utc>) {
        self.rows
            .lock()
            .expect("the store lock is not poisoned")
            .write(id, Arc::new(state), modified_at);
    }

    /// Hold a Temporal Evolution of the entity `id` names without a current
    /// state, at `at`.
    pub(crate) fn seed_evolution(&self, id: &Identifier, at: DateTime<Utc>) {
        self.rows.lock().expect("the store lock is not poisoned").retire(id, at);
    }

    /// Have `writer` interfere with the commits that follow.
    pub(crate) fn interfere(&self, writer: ConcurrentWriter) {
        *self.concurrent.lock().expect("the store lock is not poisoned") = Some(writer);
    }

    /// Have `writer` land just before the next read, as a write another
    /// replica commits after an operation read its clock and before it read
    /// the entity.
    pub(crate) fn interfere_before_read(&self, writer: impl FnOnce(&mut StoredRows) + Send + 'static) {
        *self.before_read.lock().expect("the store lock is not poisoned") = Some(Box::new(writer));
    }

    /// The latest instant of the Temporal Evolution the entity `id` names
    /// holds without a current state.
    pub(crate) fn evolution_at(&self, id: &Identifier) -> Option<DateTime<Utc>> {
        self.rows.lock().expect("the store lock is not poisoned").evolution_at(id)
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
        self.rows.lock().expect("the store lock is not poisoned").row(id).cloned()
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

    /// The row of the entity `id` names, counted as a state read, after the
    /// writer set to land before it.
    fn read(&self, id: &Identifier) -> Option<StoredRow> {
        let before_read = self.before_read.lock().expect("the store lock is not poisoned").take();
        if let Some(write) = before_read {
            write(&mut self.rows.lock().expect("the store lock is not poisoned"));
        }
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
        Ok(self.read(id).map(snapshot_of))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.read(id).map(|row| {
                    (
                        id.clone(), // clone: the answer owns the identifiers it is keyed by
                        snapshot_of(row),
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

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Ok(ids
            .iter()
            // clone: the answer owns the identifiers it is keyed by
            .filter_map(|id| self.modified_at(id).map(|modified_at| (id.clone(), sys_attrs(modified_at))))
            .collect())
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
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    /// The instant every write of a test is accepted at.
    fn accepted_at() -> SystemInstant {
        SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 28, 12, 0, 0).single().expect("a valid instant"))
    }

    /// An instant `seconds` after [`accepted_at`], or before it when negative.
    fn offset(seconds: i64) -> DateTime<Utc> {
        accepted_at().get() + TimeDelta::seconds(seconds)
    }

    /// An update of `previous` to the Building `id` holding `attributes`,
    /// stated against `read`.
    fn update(id: &Identifier, previous: &Arc<EntityState>, attributes: &[&str], read: EntityVersion) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.clone()))
            .entity(EntityMutation::Update(
                EntityRecord::replacing(previous, Arc::new(stored_building(id, attributes))).expect("a projectable state"),
            ))
            .precondition(Precondition::UnchangedSince(read))
            .build()
    }

    /// The creation of the Building `id` holding `attributes`.
    fn create(id: &Identifier, attributes: &[&str]) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.clone()))
            .entity(EntityMutation::Insert(
                EntityRecord::from_state(Arc::new(stored_building(id, attributes))).expect("a projectable state"),
            ))
            .precondition(Precondition::MustNotExist)
            .build()
    }

    /// The deletion of the Building `id`, stated against `read`.
    fn delete(id: &Identifier, read: EntityVersion) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.clone()))
            .entity(EntityMutation::Delete)
            .precondition(Precondition::UnchangedSince(read))
            .build()
    }

    /// A write that changes no current state of the Building `id`, stated
    /// against `read`, as a backfill folded against `read` sends.
    fn backfill(id: &Identifier, read: EntityVersion) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.clone()))
            .entity(EntityMutation::None)
            .precondition(Precondition::UnchangedSince(read))
            .build()
    }

    /// Commit `write` at the shared instant and answer its one outcome.
    async fn commit(store: &InMemoryStore, write: AtomicWrite) -> WriteOutcome {
        let mut outcomes = store.commit(&ContextTarget::Live, accepted_at(), vec![write]).await.expect("the commit runs");
        outcomes.pop().expect("one outcome per write")
    }

    /// The Building `a`, stored with `seed` at `modified_at`, and its version.
    fn seeded_at(store: &InMemoryStore, modified_at: DateTime<Utc>) -> (Identifier, Arc<EntityState>, EntityVersion) {
        let id = building_id("a");
        store.seed_at(&id, stored_building(&id, &["seed"]), modified_at);
        let (state, version) = store.stored(&id).expect("the Building is stored");
        (id, state, version)
    }

    /// Writer `b` read the version `a` left and committed in the same instant
    /// as `a`, so both stamped the same `modifiedAt`. A reader holding `a`'s
    /// version still loses its compare-and-set, and `b`'s change survives.
    #[tokio::test]
    async fn a_write_stating_a_version_replaced_in_the_same_instant_is_superseded() {
        let store = InMemoryStore::default();
        let (id, seeded, seeded_version) = seeded_at(&store, offset(-60));

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
        let (id, first, first_version) = seeded_at(&store, offset(-60));

        assert!(commit(&store, delete(&id, first_version)).await.is_deleted());
        store.seed_at(&id, stored_building(&id, &["second"]), offset(-30));
        let (_, second_version) = store.stored(&id).expect("the Building is stored again");
        assert_eq!(second_version.write_count(), first_version.write_count());

        assert!(commit(&store, update(&id, &first, &["stale"], first_version)).await.is_superseded());
        let (stored, _) = store.stored(&id).expect("the Building is stored");
        assert_eq!(attribute_terms(&stored), vec!["second"]);
    }

    /// A backfill folded against a version a deletion replaced is superseded,
    /// so it is folded again rather than recorded against a state that is
    /// gone; one against the standing version lands.
    #[tokio::test]
    async fn a_write_changing_no_current_state_is_held_to_the_version_it_read() {
        let store = InMemoryStore::default();
        let (id, _, version) = seeded_at(&store, offset(-60));

        assert!(commit(&store, backfill(&id, version)).await.is_no_entity_change());
        assert!(commit(&store, delete(&id, version)).await.is_deleted());
        assert!(commit(&store, backfill(&id, version)).await.is_superseded());
    }

    /// Clause 4.8: a write stamped before the `modifiedAt` the entity holds
    /// would move it backwards, so it is refused with that instant and the
    /// entity keeps its state and its version.
    #[tokio::test]
    async fn a_write_stamped_before_the_entity_s_modified_at_is_predated_by_it() {
        let store = InMemoryStore::default();
        let (id, seeded, version) = seeded_at(&store, offset(5));

        let outcome = commit(&store, update(&id, &seeded, &["late"], version)).await;
        assert!(matches!(outcome, WriteOutcome::Predated(latest) if latest.get() == offset(5)), "{outcome:?}");
        assert!(matches!(commit(&store, delete(&id, version)).await, WriteOutcome::Predated(_)));
        assert_eq!(store.stored(&id).map(|(_, stored)| stored), Some(version));
        assert_eq!(store.modified_at(&id), Some(offset(5)));
    }

    /// One batch stamps one `modifiedAt` on every instance of a repeated
    /// identifier (clause 5.5.11.0), so a write at the entity's own instant
    /// lands.
    #[tokio::test]
    async fn a_write_at_the_entity_s_modified_at_lands() {
        let store = InMemoryStore::default();
        let (id, seeded, version) = seeded_at(&store, accepted_at().get());
        assert!(commit(&store, update(&id, &seeded, &["again"], version)).await.is_updated());
    }

    /// Clause 5.6.6 leaves the Temporal Evolution standing at the deletion's
    /// instant, and a creation stamped before it would record its instances
    /// before the tombstones of their predecessors.
    #[tokio::test]
    async fn a_creation_stamped_before_a_deletion_of_the_identifier_is_predated_by_it() {
        let store = InMemoryStore::default();
        let id = building_id("a");
        store.seed_evolution(&id, offset(5));

        let outcome = commit(&store, create(&id, &["created"])).await;
        assert!(matches!(outcome, WriteOutcome::Predated(latest) if latest.get() == offset(5)), "{outcome:?}");
        assert!(store.stored(&id).is_none());

        store.seed_evolution(&id, offset(-5));
        assert!(commit(&store, create(&id, &["created"])).await.is_created());
        assert!(store.evolution_at(&id).is_none());
    }

    #[tokio::test]
    async fn a_deletion_leaves_the_evolution_at_its_instant() {
        let store = InMemoryStore::default();
        let (id, _, version) = seeded_at(&store, offset(-60));
        assert!(commit(&store, delete(&id, version)).await.is_deleted());
        assert_eq!(store.evolution_at(&id), Some(accepted_at().get()));
    }
}
