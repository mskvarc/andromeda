//! The closed set of relations that maintenance may inspect or mutate.

pub mod heap_target;
pub mod index_target;

use crate::candidate::{heap_target::HeapTarget, index_target::IndexTarget};

/// A persistable allowlisted maintenance target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaintenanceTarget {
    /// One allowlisted index.
    Index(IndexTarget),
    /// One hot or queue heap.
    Heap(HeapTarget),
    /// The planner statistics of one heap.
    Statistics(HeapTarget),
}

impl MaintenanceTarget {
    /// The exact database identifier of the target, a bounded-cardinality
    /// label for logs.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            MaintenanceTarget::Index(target) => target.relation_name(),
            MaintenanceTarget::Heap(target) | MaintenanceTarget::Statistics(target) => target.relation_name(),
        }
    }

    /// The heap whose load and lock gates apply to the target: the target
    /// itself, the heap an index belongs to, or the heap whose statistics it
    /// is.
    #[must_use]
    pub const fn heap(self) -> HeapTarget {
        match self {
            MaintenanceTarget::Index(target) => target.heap(),
            MaintenanceTarget::Heap(target) | MaintenanceTarget::Statistics(target) => target,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::{MaintenanceTarget, heap_target::HeapTarget, index_target::IndexTarget},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_snapshot_branch_action_runs_in_the_branch_schema() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(42).expect("positive snapshot key"));
        assert_eq!(
            IndexTarget::IdxCaValueNumber.reindex_statement(branch).as_sql(),
            "REINDEX INDEX CONCURRENTLY \"snap_42\".\"idx_ca_value_number\""
        );
        assert_eq!(
            HeapTarget::CurrentAttributes.vacuum_statement(branch).as_sql(),
            "VACUUM (TRUNCATE FALSE, PARALLEL 0) \"snap_42\".\"current_attributes\""
        );
        assert_eq!(
            HeapTarget::EntitySearch.analyze_statement(branch).as_sql(),
            "ANALYZE \"snap_42\".\"entity_search\""
        );
    }

    #[test]
    fn every_index_target_names_the_heap_that_owns_it() {
        let expected = [
            (IndexTarget::UqCurrentAttributesGrain, HeapTarget::CurrentAttributes),
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
            (IndexTarget::IdxNotificationDeliveriesEvent, HeapTarget::NotificationDeliveries),
            (IndexTarget::IdxNotificationDeliveriesSubscription, HeapTarget::NotificationDeliveries),
            (IndexTarget::PkNotificationDeliveries, HeapTarget::NotificationDeliveries),
        ];
        assert_eq!(IndexTarget::ALL.map(|target| (target, target.heap())), expected);
        for (index, heap) in expected {
            assert_eq!(MaintenanceTarget::Index(index).heap(), heap, "{index}");
        }
        for heap in HeapTarget::ALL {
            assert_eq!(MaintenanceTarget::Heap(heap).heap(), heap);
            assert_eq!(MaintenanceTarget::Statistics(heap).heap(), heap);
        }
    }
}
