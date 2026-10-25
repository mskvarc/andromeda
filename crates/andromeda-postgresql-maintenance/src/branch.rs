//! The context branch a maintenance target lives in.
//!
//! A tenant database holds the live branch in `public` and one published
//! schema per Snapshot branch. The schema name of a Snapshot branch belongs to
//! the branch crate; this module only maps a target's branch to it and back.
//! A Snapshot's private staging schema is owned by its preparation and is
//! never a maintenance branch.

use crate::{
    candidate::{
        MaintenanceTarget,
        heap_target::{HeapTarget, RelationPresence},
    },
    error::MaintenanceError,
    observed_heap::WatchedHeap,
};
use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::{published_snapshot_key, snapshot_schema_name};
use andromeda_postgresql_seaquery::maintenance::MaintenanceSchema;
use sea_query::{Alias, Iden};

/// One context branch of a tenant database.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Branch {
    /// The live branch in the application schema.
    Live,
    /// The published branch of one Snapshot.
    Snapshot(SnapshotKey),
}

impl Branch {
    /// The branch a catalog schema name denotes, or `None` for a schema that
    /// is no maintenance branch (a staging schema included).
    pub(crate) fn from_schema(schema: &str) -> Option<Branch> {
        if MaintenanceSchema::Public.to_string() == schema {
            Some(Branch::Live)
        } else {
            published_snapshot_key(schema).map(Branch::Snapshot)
        }
    }

    /// The schema identifier statements on this branch are qualified with.
    pub(crate) fn schema(self) -> Alias {
        Alias::new(self.schema_name())
    }

    /// The name of this branch's schema, as the catalogs report it.
    pub(crate) fn schema_name(self) -> String {
        match self {
            Branch::Live => MaintenanceSchema::Public.to_string(),
            Branch::Snapshot(key) => snapshot_schema_name(key, SnapshotBranch::Published),
        }
    }

    /// `schema.relation`, the name a `regclass` lookup resolves a relation of
    /// this branch by. Both parts are unquoted identifiers: the schema name is
    /// `public` or derived from a positive key, the relation an allowlisted name.
    pub(crate) fn qualified(self, relation: &str) -> String {
        format!("{}.{relation}", self.schema_name())
    }

    /// Whether this branch's schema holds a relation present in `presence`:
    /// the live branch holds every relation, a Snapshot branch only the branch
    /// relations.
    const fn holds(self, presence: RelationPresence) -> bool {
        match (self, presence) {
            (Branch::Live, RelationPresence::EveryBranch | RelationPresence::LiveOnly) | (Branch::Snapshot(_), RelationPresence::EveryBranch) => true,
            (Branch::Snapshot(_), RelationPresence::LiveOnly) => false,
        }
    }

    /// The heap targets this branch's schema holds.
    pub(crate) fn heaps(self) -> impl Iterator<Item = HeapTarget> {
        HeapTarget::ALL.into_iter().filter(move |heap| self.holds(heap.presence()))
    }

    /// The heaps whose autovacuum and statistics health a visit reads in this
    /// branch: its heap targets and the heaps it only observes.
    pub(crate) fn watched_heaps(self) -> impl Iterator<Item = WatchedHeap> {
        WatchedHeap::all().filter(move |heap| self.holds(heap.presence()))
    }

    /// The Snapshot whose branch this is, `None` for the live branch.
    pub(crate) const fn snapshot_key(self) -> Option<SnapshotKey> {
        match self {
            Branch::Live => None,
            Branch::Snapshot(key) => Some(key),
        }
    }
}

/// A maintenance target in one context branch whose schema holds it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BranchTarget {
    /// The branch.
    branch: Branch,
    /// The target.
    target: MaintenanceTarget,
}

impl BranchTarget {
    /// A target in `branch`.
    ///
    /// # Errors
    ///
    /// Returns [`MaintenanceError::LiveOnlyTarget`] when `branch` is a
    /// Snapshot branch and the target exists only in the live branch.
    pub(crate) const fn new(branch: Branch, target: MaintenanceTarget) -> Result<BranchTarget, MaintenanceError> {
        match (branch, target.presence()) {
            (Branch::Snapshot(snapshot), RelationPresence::LiveOnly) => Err(MaintenanceError::LiveOnlyTarget { target, snapshot }),
            (Branch::Live, RelationPresence::EveryBranch | RelationPresence::LiveOnly) | (Branch::Snapshot(_), RelationPresence::EveryBranch) => {
                Ok(BranchTarget { branch, target })
            }
        }
    }

    /// The branch.
    pub(crate) const fn branch(self) -> Branch {
        self.branch
    }

    /// The target.
    pub(crate) const fn target(self) -> MaintenanceTarget {
        self.target
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget},
        error::MaintenanceError,
        observed_heap::WatchedHeap,
    };
    use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
    use andromeda_postgresql_branch::snapshot_schema_name;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    fn key(raw: i64) -> SnapshotKey {
        SnapshotKey::try_from(raw).expect("positive snapshot key")
    }

    #[test]
    fn branch_schema_name_comes_from_the_snapshot_key() {
        assert_eq!(
            Branch::Snapshot(key(42)).schema().to_string(),
            snapshot_schema_name(key(42), SnapshotBranch::Published)
        );
        assert_eq!(Branch::Live.schema().to_string(), "public");
    }

    #[test]
    fn staging_schema_is_never_a_branch() {
        assert_eq!(Branch::from_schema("snap_7_stg"), None);
        assert_eq!(Branch::from_schema("snap_7"), Some(Branch::Snapshot(key(7))));
        assert_eq!(Branch::from_schema("public"), Some(Branch::Live));
        assert_eq!(Branch::from_schema("pg_catalog"), None);
    }

    #[test]
    fn live_only_tables_are_never_branch_targets() {
        for heap in [
            HeapTarget::PendingSideEffects,
            HeapTarget::TemporalAppendIntents,
            HeapTarget::SnapshotNotificationOutbox,
        ] {
            for target in [MaintenanceTarget::Heap(heap), MaintenanceTarget::Statistics(heap)] {
                assert!(
                    matches!(
                        BranchTarget::new(Branch::Snapshot(key(7)), target),
                        Err(MaintenanceError::LiveOnlyTarget { target: rejected, snapshot }) if rejected == target && snapshot == key(7)
                    ),
                    "{heap}"
                );
                assert!(BranchTarget::new(Branch::Live, target).is_ok(), "{heap}");
            }
        }
    }

    #[test]
    fn a_history_hypertable_is_never_recompressed_in_a_snapshot_branch() {
        let target = MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory);
        assert!(matches!(
            BranchTarget::new(Branch::Snapshot(key(7)), target),
            Err(MaintenanceError::LiveOnlyTarget { target: rejected, snapshot }) if rejected == target && snapshot == key(7)
        ));
        assert!(BranchTarget::new(Branch::Live, target).is_ok());
    }

    #[test]
    fn a_snapshot_branch_holds_only_the_branch_heaps() {
        assert_eq!(Branch::Live.heaps().collect::<Vec<_>>(), HeapTarget::ALL);
        assert_eq!(
            Branch::Snapshot(key(7)).heaps().collect::<Vec<_>>(),
            [
                HeapTarget::CurrentAttributes,
                HeapTarget::Entities,
                HeapTarget::SubscriptionDeliveryState,
                HeapTarget::NotificationEvents,
                HeapTarget::NotificationDeliveries,
                HeapTarget::EntitySearch,
            ]
        );
        assert_eq!(Branch::Snapshot(key(7)).qualified("entities"), "snap_7.entities");
        let watched = Branch::Snapshot(key(7)).watched_heaps().map(WatchedHeap::relation_name).collect::<Vec<_>>();
        assert!(watched.contains(&"attribute_census"), "{watched:?}");
        assert!(!watched.contains(&"snapshot_preparation"), "{watched:?}");
        assert!(!watched.contains(&"pending_side_effects"), "{watched:?}");
        assert!(Branch::Live.watched_heaps().any(|heap| heap.relation_name() == "snapshot_preparation"));
        assert_eq!(Branch::Live.qualified("entities"), "public.entities");
    }

    #[test]
    fn branch_relations_are_targets_in_every_branch() {
        for target in [
            MaintenanceTarget::Heap(HeapTarget::CurrentAttributes),
            MaintenanceTarget::Heap(HeapTarget::Entities),
            MaintenanceTarget::Heap(HeapTarget::SubscriptionDeliveryState),
            MaintenanceTarget::Heap(HeapTarget::NotificationEvents),
            MaintenanceTarget::Heap(HeapTarget::NotificationDeliveries),
            MaintenanceTarget::Heap(HeapTarget::EntitySearch),
            MaintenanceTarget::Statistics(HeapTarget::EntitySearch),
            MaintenanceTarget::Index(IndexTarget::IdxCaValueNumber),
        ] {
            let in_branch = BranchTarget::new(Branch::Snapshot(key(7)), target).expect("a branch relation is a branch target");
            assert_eq!((in_branch.branch(), in_branch.target()), (Branch::Snapshot(key(7)), target));
        }
    }
}
