//! The closed set of relations that maintenance may inspect or mutate.

pub mod heap_target;
pub mod history_table;
pub mod index_target;

use crate::candidate::{
    heap_target::{HeapTarget, RelationPresence},
    history_table::HistoryTable,
    index_target::IndexTarget,
};

/// A persistable allowlisted maintenance target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaintenanceTarget {
    /// One allowlisted index.
    Index(IndexTarget),
    /// One hot or queue heap.
    Heap(HeapTarget),
    /// The planner statistics of one heap.
    Statistics(HeapTarget),
    /// The partially converted chunks of one history hypertable, recompressed
    /// one chunk at a time, the oldest first.
    Columnstore(HistoryTable),
    /// The closed rowstore chunks of one history hypertable, each chunk's
    /// indexes rebuilt once, the oldest chunk first.
    Rowstore(HistoryTable),
}

impl MaintenanceTarget {
    /// The exact database identifier of the target, a bounded-cardinality
    /// label for logs.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            MaintenanceTarget::Index(target) => target.relation_name(),
            MaintenanceTarget::Heap(target) | MaintenanceTarget::Statistics(target) => target.relation_name(),
            MaintenanceTarget::Columnstore(table) | MaintenanceTarget::Rowstore(table) => table.relation_name(),
        }
    }

    /// The context branches whose schema holds the target: those holding its
    /// heap, or the heap of the index, or only the live branch for a history
    /// hypertable, since a Snapshot branch holds its history as a plain table.
    #[must_use]
    pub const fn presence(self) -> RelationPresence {
        match self {
            MaintenanceTarget::Index(target) => target.heap().presence(),
            MaintenanceTarget::Heap(target) | MaintenanceTarget::Statistics(target) => target.presence(),
            MaintenanceTarget::Columnstore(_) | MaintenanceTarget::Rowstore(_) => RelationPresence::LiveOnly,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::{
            MaintenanceTarget,
            heap_target::{HeapTarget, RelationPresence},
            history_table::HistoryTable,
            index_target::IndexTarget,
        },
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_seaquery::maintenance::{analyze, reindex_index_concurrently, vacuum_non_truncating};
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn a_snapshot_branch_action_runs_in_the_branch_schema() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(42).expect("positive snapshot key"));
        assert_eq!(
            IndexTarget::IdxCaValueNumber.reindex_statement(branch),
            reindex_index_concurrently(Alias::new("snap_42"), Alias::new("idx_ca_value_number"))
        );
        assert_eq!(
            HeapTarget::CurrentAttributes.vacuum_statement(branch),
            vacuum_non_truncating(Alias::new("snap_42"), Alias::new("current_attributes"))
        );
        assert_eq!(
            HeapTarget::EntitySearch.analyze_statement(branch),
            analyze(Alias::new("snap_42"), Alias::new("entity_search"))
        );
    }

    #[test]
    fn every_index_target_names_the_heap_that_owns_it() {
        let expected = [
            (IndexTarget::PkCaGrain, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueNumber, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueString, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueStringTrgm, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueBoolean, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueDateTime, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueUri, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueGeometry, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaEntityGeometry, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaSubEqKeys, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueObject, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueLongTextTrgm, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxNotificationDeliveriesClaim, HeapTarget::NotificationDeliveries),
            (IndexTarget::UqNotificationDeliveriesEventSubscription, HeapTarget::NotificationDeliveries),
            (IndexTarget::IdxNotificationDeliveriesSubscription, HeapTarget::NotificationDeliveries),
            (IndexTarget::PkNotificationDeliveries, HeapTarget::NotificationDeliveries),
            (IndexTarget::IdxEntitiesIdTrgm, HeapTarget::Entities),
            (IndexTarget::PkTemporalAttributeStreams, HeapTarget::TemporalAttributeStreams),
            (IndexTarget::UqTemporalAttributeStreamsGrain, HeapTarget::TemporalAttributeStreams),
            (
                IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey,
                HeapTarget::TemporalAttributeStreams,
            ),
            (IndexTarget::IdxTemporalAttributeStreamsValueExtent, HeapTarget::TemporalAttributeStreams),
            (
                IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask,
                HeapTarget::TemporalAttributeStreams,
            ),
        ];
        assert_eq!(IndexTarget::ALL.map(|target| (target, target.heap())), expected);
        for (index, heap) in expected {
            assert_eq!(MaintenanceTarget::Index(index).presence(), heap.presence(), "{index}");
        }
        for heap in HeapTarget::ALL {
            assert_eq!(MaintenanceTarget::Heap(heap).presence(), heap.presence());
            assert_eq!(MaintenanceTarget::Statistics(heap).presence(), heap.presence());
        }
    }

    #[test]
    fn a_history_hypertable_is_a_target_of_the_live_branch_alone() {
        for table in HistoryTable::ALL {
            assert_eq!(MaintenanceTarget::Columnstore(table).presence(), RelationPresence::LiveOnly);
            assert_eq!(MaintenanceTarget::Columnstore(table).relation_name(), table.relation_name());
        }
    }
}
