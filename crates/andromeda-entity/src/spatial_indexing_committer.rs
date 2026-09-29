//! Keeping the entity spatial index in step with every live commit.
//!
//! Under `commit-synchronous` consistency the index is updated on the commit
//! path, before the write that changed an entity's location returns. The
//! update is made here, around the commit port every current-state write goes
//! through -- entity, batch, purge and expiry writes as much as the placeholder
//! a temporal write creates -- rather than at each of their call sites, so no
//! write path can be left out.
//!
//! Three properties keep the index from ever missing an entity the store holds:
//!
//! - Before the commit, the entity's new geometry is *added* to the index
//!   beside its current one. The index therefore holds the entity wherever the
//!   commit may leave it, including when the commit future is dropped after the
//!   store committed but before the outcome was read.
//! - After the commit, a landed write *replaces* the entity's entries with its
//!   new state, and a landed deletion removes them. A write that did not land
//!   leaves the added geometry behind, which can only widen a candidate set:
//!   the store re-verifies every candidate the index hands it.
//! - The commit and its index update run under a per-entity lock, so two
//!   writes of one entity are applied to the index in the order the store
//!   committed them.

use crate::{
    id_lock::{IdLockMap, acquire_id_locks_in},
    spatial_placement::LocationPlacement,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    entity_mutation::EntityMutation,
    error::CommitError,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_spatial_index::{entity::index::SpatialEntityIndex, key::SpatialKey};
use dashmap::DashMap;
use std::sync::Arc;

/// A commit port that updates the live branch's spatial index around every
/// commit it forwards.
pub struct SpatialIndexingCommitter {
    /// The commit port every write is forwarded to.
    inner: Arc<dyn ContextCommitter>,
    /// The live branch's spatial index.
    index: Arc<SpatialEntityIndex>,
    /// Which partitions an entity's location occupies.
    placement: LocationPlacement,
    /// Per-entity locks ordering one entity's commits and index updates.
    locks: IdLockMap,
}

impl SpatialIndexingCommitter {
    /// Keep `index` in step with every live commit forwarded to `inner`.
    #[must_use]
    pub fn new(inner: Arc<dyn ContextCommitter>, index: Arc<SpatialEntityIndex>, placement: LocationPlacement) -> SpatialIndexingCommitter {
        SpatialIndexingCommitter {
            inner,
            index,
            placement,
            locks: DashMap::new(),
        }
    }

    /// Forward one commit, updating the index around it when it targets the
    /// live branch.
    async fn forward(&self, target: &ContextTarget, writes: Vec<AtomicWrite>, replay: Option<CommitReplay>) -> Result<Vec<WriteOutcome>, CommitError> {
        // A Snapshot branch is never indexed: its queries always go to the store.
        if target.snapshot().is_some() {
            return self.commit_inner(target, writes, replay).await;
        }
        let changes: Vec<IndexChange> = writes.iter().map(|write| IndexChange::of(write, &self.placement)).collect();
        let entities: Vec<Identifier> = changes.iter().filter_map(IndexChange::entity).cloned().collect(); // clone: the lock map owns the identifiers it locks
        let _guards = acquire_id_locks_in(&self.locks, target, &entities).await;
        for change in &changes {
            change.anticipate(&self.index);
        }
        let outcomes = self.commit_inner(target, writes, replay).await?;
        for (change, outcome) in changes.into_iter().zip(&outcomes) {
            change.settle(&self.index, outcome);
        }
        Ok(outcomes)
    }

    /// Forward one commit to the wrapped port.
    async fn commit_inner(&self, target: &ContextTarget, writes: Vec<AtomicWrite>, replay: Option<CommitReplay>) -> Result<Vec<WriteOutcome>, CommitError> {
        match replay {
            Some(replay) => self.inner.commit_replayed(target, replay, writes).await,
            None => self.inner.commit(target, writes).await,
        }
    }
}

impl ContextCommitter for SpatialIndexingCommitter {
    fn commit(&self, target: &ContextTarget, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone(); // clone: the owned future outlives the borrowed target
        Box::pin(async move { self.forward(&target, writes, None).await })
    }

    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone(); // clone: the owned future outlives the borrowed target
        Box::pin(async move { self.forward(&target, writes, Some(replay)).await })
    }
}

/// What one write does to the index once its outcome is known.
enum IndexChange {
    /// The write states the entity's whole current state.
    Restate {
        /// The entity written.
        entity_id: Identifier,
        /// The partitions the entity's location occupies once the write lands.
        placements: Vec<(SpatialKey, Geometry)>,
    },
    /// The write deletes the entity.
    Remove {
        /// The entity deleted.
        entity_id: Identifier,
    },
    /// The write leaves the entity's current state alone.
    Unchanged,
}

impl IndexChange {
    /// The index change one write makes if it lands.
    fn of(write: &AtomicWrite, placement: &LocationPlacement) -> IndexChange {
        let entity_id = || (*write.entity_id).clone(); // clone: the index owns the identifier the write shares
        match &write.entity {
            EntityMutation::Insert(record) | EntityMutation::Update(record) => IndexChange::Restate {
                entity_id: entity_id(),
                placements: placement.of(&record.projection.types, record.projection.location.as_ref()),
            },
            EntityMutation::Delete => IndexChange::Remove { entity_id: entity_id() },
            EntityMutation::None => IndexChange::Unchanged,
        }
    }

    /// The entity this change concerns, if it concerns one.
    const fn entity(&self) -> Option<&Identifier> {
        match self {
            IndexChange::Restate { entity_id, .. } | IndexChange::Remove { entity_id } => Some(entity_id),
            IndexChange::Unchanged => None,
        }
    }

    /// Make the entity reachable wherever the write may leave it, before the
    /// write commits.
    fn anticipate(&self, index: &SpatialEntityIndex) {
        match self {
            IndexChange::Restate { entity_id, placements } => {
                // clone: the settled replacement needs the same identifier and placements once the outcome is known
                index.anticipate_entity(entity_id.clone(), placements.clone());
            }
            // Keeping the entity's current entries until the deletion lands is
            // what keeps it reachable if the deletion does not.
            IndexChange::Remove { .. } | IndexChange::Unchanged => {}
        }
    }

    /// Settle the index to what the write actually did.
    fn settle(self, index: &SpatialEntityIndex, outcome: &WriteOutcome) {
        match (self, outcome) {
            (IndexChange::Restate { entity_id, placements }, WriteOutcome::Created | WriteOutcome::Updated) => index.replace_entity(entity_id, placements),
            (IndexChange::Remove { entity_id }, WriteOutcome::Deleted(_)) => index.remove(&entity_id),
            // A write that did not land leaves the store as it was; whatever it
            // anticipated only widens a candidate set until the entity's next
            // write or the next rebuild.
            (
                IndexChange::Restate { .. } | IndexChange::Remove { .. } | IndexChange::Unchanged,
                WriteOutcome::Created
                | WriteOutcome::Updated
                | WriteOutcome::Deleted(_)
                | WriteOutcome::AlreadyExists
                | WriteOutcome::NotFound
                | WriteOutcome::Superseded
                | WriteOutcome::Skipped
                | WriteOutcome::NoEntityChange,
            ) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{spatial_indexing_committer::SpatialIndexingCommitter, spatial_placement::LocationPlacement};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        commit_replay::CommitReplay,
        committer::{CommitFuture, ContextCommitter},
        deleted_entity::DeletedEntity,
        entity_mutation::EntityMutation,
        entity_record::{EntityRecord, SearchProjection},
        error::CommitError,
        write_outcome::WriteOutcome,
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_geometry::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        geometry::Geometry,
        position::Position,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_spatial_index::{entity::index::SpatialEntityIndex, key::SpatialKey};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::Utc;
    use futures::{FutureExt, future::pending};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    /// What the scripted store does with every commit it is handed.
    enum Script {
        /// Every write lands with this outcome.
        Answer(fn() -> WriteOutcome),
        /// The commit never completes, as when the request is dropped while
        /// the store is committing.
        Hang,
    }

    /// A store answering every commit by its script.
    struct ScriptedStore(Script);

    impl ContextCommitter for ScriptedStore {
        fn commit(&self, _target: &ContextTarget, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            match self.0 {
                Script::Answer(outcome) => Box::pin(async move { Ok(writes.iter().map(|_| outcome()).collect()) }),
                Script::Hang => Box::pin(pending()),
            }
        }

        fn commit_replayed(&self, target: &ContextTarget, _replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            self.commit(target, writes)
        }
    }

    fn vehicle() -> Identifier {
        "urn:ngsi-ld:Vehicle:A".parse().expect("a well-formed URI")
    }

    fn point(x: f64) -> Geometry {
        Geometry::new(GeometryForm::Point(Position::new(x, 0.0, None)))
    }

    fn located_at(x: f64) -> EntityMutation {
        let projection = SearchProjection {
            types: vec!["https://example.org/Vehicle".parse().expect("a well-formed type")],
            location: Some(point(x)),
            ..SearchProjection::default()
        };
        EntityMutation::Update(EntityRecord::from_parts(Arc::new(json!([{}])), projection, None, None))
    }

    fn write(entity: EntityMutation) -> AtomicWrite {
        AtomicWrite::builder().entity_id(Arc::new(vehicle())).entity(entity).build()
    }

    fn seeded_index() -> Arc<SpatialEntityIndex> {
        let index = Arc::new(SpatialEntityIndex::new());
        let placement = LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name");
        index.rebuild(placement.tenant(), placement.geoproperty(), HashMap::new(), index.current_generation());
        index
    }

    fn committer(index: &Arc<SpatialEntityIndex>, script: Script) -> SpatialIndexingCommitter {
        SpatialIndexingCommitter::new(
            Arc::new(ScriptedStore(script)),
            Arc::clone(index), // clone: the committer and the test share the index
            LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name"),
        )
    }

    /// The vehicles the index holds within one degree of `(x, 0)`.
    fn around(index: &SpatialEntityIndex, x: f64) -> Option<HashSet<Identifier>> {
        let key = SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::new("https://example.org/Vehicle".to_owned()).expect("a well-formed IRI"),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        );
        let square = Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![vec![
            Position::new(x - 1.0, -1.0, None),
            Position::new(x + 1.0, -1.0, None),
            Position::new(x + 1.0, 1.0, None),
            Position::new(x - 1.0, 1.0, None),
            Position::new(x - 1.0, -1.0, None),
        ]])));
        index.query(&key, &GeoRel::Within, &square)
    }

    fn deleted() -> WriteOutcome {
        WriteOutcome::Deleted(DeletedEntity {
            id: vehicle(),
            payload: json!([{}]),
            created_at: Utc::now(),
            modified_at: Utc::now(),
            expires_at: None,
        })
    }

    #[tokio::test]
    async fn a_landed_write_is_in_the_index_before_the_commit_returns() {
        let index = seeded_index();
        let committer = committer(&index, Script::Answer(|| WriteOutcome::Updated));
        committer
            .commit(&ContextTarget::Live, vec![write(located_at(0.0))])
            .await
            .expect("the write lands");
        assert_eq!(around(&index, 0.0), Some(HashSet::from([vehicle()])));
    }

    #[tokio::test]
    async fn a_landed_move_leaves_the_old_location() {
        let index = seeded_index();
        let committer = committer(&index, Script::Answer(|| WriteOutcome::Updated));
        committer
            .commit(&ContextTarget::Live, vec![write(located_at(0.0))])
            .await
            .expect("the write lands");
        committer
            .commit(&ContextTarget::Live, vec![write(located_at(20.0))])
            .await
            .expect("the move lands");
        assert_eq!(around(&index, 0.0), Some(HashSet::new()));
        assert_eq!(around(&index, 20.0), Some(HashSet::from([vehicle()])));
    }

    #[tokio::test]
    async fn a_superseded_write_keeps_the_entity_where_the_store_still_has_it() {
        let index = seeded_index();
        committer(&index, Script::Answer(|| WriteOutcome::Updated))
            .commit(&ContextTarget::Live, vec![write(located_at(0.0))])
            .await
            .expect("the write lands");
        committer(&index, Script::Answer(|| WriteOutcome::Superseded))
            .commit(&ContextTarget::Live, vec![write(located_at(20.0))])
            .await
            .expect("the commit answers");
        assert_eq!(around(&index, 0.0), Some(HashSet::from([vehicle()])));
    }

    #[tokio::test]
    async fn a_commit_dropped_before_its_outcome_leaves_the_entity_reachable_at_its_new_location() {
        let index = seeded_index();
        committer(&index, Script::Answer(|| WriteOutcome::Updated))
            .commit(&ContextTarget::Live, vec![write(located_at(0.0))])
            .await
            .expect("the write lands");
        let hanging = committer(&index, Script::Hang);
        assert!(hanging.commit(&ContextTarget::Live, vec![write(located_at(20.0))]).now_or_never().is_none());
        assert_eq!(around(&index, 0.0), Some(HashSet::from([vehicle()])));
        assert_eq!(around(&index, 20.0), Some(HashSet::from([vehicle()])));
    }

    #[tokio::test]
    async fn a_landed_deletion_removes_the_entity_and_a_refused_one_keeps_it() {
        let index = seeded_index();
        committer(&index, Script::Answer(|| WriteOutcome::Updated))
            .commit(&ContextTarget::Live, vec![write(located_at(0.0))])
            .await
            .expect("the write lands");
        committer(&index, Script::Answer(|| WriteOutcome::NotFound))
            .commit(&ContextTarget::Live, vec![write(EntityMutation::Delete)])
            .await
            .expect("the commit answers");
        assert_eq!(around(&index, 0.0), Some(HashSet::from([vehicle()])));
        committer(&index, Script::Answer(deleted))
            .commit(&ContextTarget::Live, vec![write(EntityMutation::Delete)])
            .await
            .expect("the deletion lands");
        assert_eq!(around(&index, 0.0), Some(HashSet::new()));
    }

    #[tokio::test]
    async fn a_snapshot_branch_write_never_reaches_the_live_index() {
        let index = seeded_index();
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(3).expect("positive key"),
            "urn:ngsi-ld:Snapshot:a".parse().expect("a well-formed URI"),
        ));
        committer(&index, Script::Answer(|| WriteOutcome::Updated))
            .commit(&snapshot, vec![write(located_at(0.0))])
            .await
            .expect("the write lands");
        assert_eq!(around(&index, 0.0), Some(HashSet::new()));
    }

    #[tokio::test]
    async fn a_failed_commit_reports_its_error() {
        struct FailingStore;
        impl ContextCommitter for FailingStore {
            fn commit(&self, _target: &ContextTarget, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
                Box::pin(async {
                    Err(CommitError::Persistence {
                        detail: "scripted failure".to_owned(),
                    })
                })
            }

            fn commit_replayed(&self, target: &ContextTarget, _replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
                self.commit(target, writes)
            }
        }
        let index = seeded_index();
        let committer = SpatialIndexingCommitter::new(
            Arc::new(FailingStore),
            Arc::clone(&index), // clone: the committer and the test share the index
            LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name"),
        );
        assert!(committer.commit(&ContextTarget::Live, vec![write(located_at(0.0))]).await.is_err());
    }
}
