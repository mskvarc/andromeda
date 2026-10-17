//! The heaps whose autovacuum and statistics health the controller watches.
//!
//! The migrations tune autovacuum for churn on more tables than the controller
//! may act on, on the assumption that autovacuum keeps up, and some untuned
//! tables churn like queues. The controller watches all of them for a missed
//! vacuum or analyze. It acts only on [`HeapTarget`]s, which need `MAINTAIN`;
//! an observed heap is promoted to a target only once evidence shows
//! autovacuum missing it.

use crate::candidate::heap_target::{HeapTarget, RelationPresence};
use std::str::FromStr;
use strum::{EnumString, IntoStaticStr, ParseError};

/// A heap the controller observes but never vacuums or analyzes itself.
#[derive(Clone, Copy, Debug, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ObservedHeap {
    /// Per-attribute counters every write touches: few rows, very hot, so a
    /// held horizon bloats them quickly.
    AttributeCensus,
    /// Per-type, per-attribute counters every write touches.
    EntityTypeAttributeCensus,
    /// Per-type counters every write touches.
    EntityTypeCensus,
    /// The entities of each type, rewritten as entities change type.
    EntityTypeMembers,
    /// The temporal entities of each type.
    TemporalEntityTypeMembers,
    /// One row per temporal attribute stream, garbage-collected by retention.
    TemporalAttributeStreams,
    /// One row per temporal entity, garbage-collected by retention.
    TemporalEntities,
    /// Verbatim `instanceId` claims, claimed and released with their rows.
    AttributeHistoryInstanceClaims,
    /// Scope changes, whose superseded rows retention deletes.
    ScopeHistory,
    /// Per-registration delivery counters, tuned with `fillfactor = 50`.
    RegistrationDeliveryState,
    /// The information entries of each registration, rewritten with it.
    RegistrationInformation,
    /// The preparation state of each Snapshot, tuned with `fillfactor = 50`.
    SnapshotPreparation,
    /// One row per write, deleted once it is released.
    WriteRecords,
}

impl ObservedHeap {
    /// Every observed heap.
    pub(crate) const ALL: [ObservedHeap; 13] = [
        ObservedHeap::AttributeCensus,
        ObservedHeap::EntityTypeAttributeCensus,
        ObservedHeap::EntityTypeCensus,
        ObservedHeap::EntityTypeMembers,
        ObservedHeap::TemporalEntityTypeMembers,
        ObservedHeap::TemporalAttributeStreams,
        ObservedHeap::TemporalEntities,
        ObservedHeap::AttributeHistoryInstanceClaims,
        ObservedHeap::ScopeHistory,
        ObservedHeap::RegistrationDeliveryState,
        ObservedHeap::RegistrationInformation,
        ObservedHeap::SnapshotPreparation,
        ObservedHeap::WriteRecords,
    ];

    /// The exact table identifier created by the migrations.
    pub(crate) fn relation_name(self) -> &'static str {
        self.into()
    }

    /// The context branches whose schema holds the heap.
    pub(crate) const fn presence(self) -> RelationPresence {
        match self {
            ObservedHeap::AttributeCensus
            | ObservedHeap::EntityTypeAttributeCensus
            | ObservedHeap::EntityTypeCensus
            | ObservedHeap::EntityTypeMembers
            | ObservedHeap::TemporalEntityTypeMembers
            | ObservedHeap::TemporalAttributeStreams
            | ObservedHeap::TemporalEntities
            | ObservedHeap::AttributeHistoryInstanceClaims
            | ObservedHeap::ScopeHistory => RelationPresence::EveryBranch,
            ObservedHeap::RegistrationDeliveryState
            | ObservedHeap::RegistrationInformation
            | ObservedHeap::SnapshotPreparation
            | ObservedHeap::WriteRecords => RelationPresence::LiveOnly,
        }
    }
}

/// A heap whose autovacuum and statistics health a visit reads.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum WatchedHeap {
    /// A heap the controller may vacuum and analyze.
    Target(HeapTarget),
    /// A heap the controller only observes.
    Observed(ObservedHeap),
}

impl WatchedHeap {
    /// The exact table identifier created by the migrations.
    pub(crate) fn relation_name(self) -> &'static str {
        match self {
            WatchedHeap::Target(heap) => heap.relation_name(),
            WatchedHeap::Observed(heap) => heap.relation_name(),
        }
    }

    /// The context branches whose schema holds the heap.
    pub(crate) const fn presence(self) -> RelationPresence {
        match self {
            WatchedHeap::Target(heap) => heap.presence(),
            WatchedHeap::Observed(heap) => heap.presence(),
        }
    }

    /// Every watched heap, the targets first.
    pub(crate) fn all() -> impl Iterator<Item = WatchedHeap> {
        HeapTarget::ALL
            .into_iter()
            .map(WatchedHeap::Target)
            .chain(ObservedHeap::ALL.into_iter().map(WatchedHeap::Observed))
    }
}

// No derive spans two enums' names, so the parse is written out.
impl FromStr for WatchedHeap {
    type Err = ParseError;

    /// The watched heap a table identifier names, a target first: no table is
    /// both.
    fn from_str(name: &str) -> Result<WatchedHeap, ParseError> {
        HeapTarget::from_str(name)
            .map(WatchedHeap::Target)
            .or_else(|_| ObservedHeap::from_str(name).map(WatchedHeap::Observed))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::heap_target::{HeapTarget, RelationPresence},
        observed_heap::{ObservedHeap, WatchedHeap},
    };
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn every_watched_heap_name_resolves_to_the_heap_it_names() {
        for heap in WatchedHeap::all() {
            assert_eq!(WatchedHeap::from_str(heap.relation_name()), Ok(heap));
        }
        assert!(WatchedHeap::from_str("attribute_history").is_err());
    }

    #[test]
    fn no_observed_heap_is_an_action_target() {
        for heap in ObservedHeap::ALL {
            assert!(HeapTarget::from_str(heap.relation_name()).is_err(), "{heap:?}");
        }
    }

    #[test]
    fn the_observed_heaps_are_the_tuned_and_churning_tables_of_the_migrations() {
        assert_eq!(
            ObservedHeap::ALL.map(ObservedHeap::relation_name),
            [
                "attribute_census",
                "entity_type_attribute_census",
                "entity_type_census",
                "entity_type_members",
                "temporal_entity_type_members",
                "temporal_attribute_streams",
                "temporal_entities",
                "attribute_history_instance_claims",
                "scope_history",
                "registration_delivery_state",
                "registration_information",
                "snapshot_preparation",
                "write_records",
            ]
        );
    }

    #[test]
    fn the_tenant_tables_are_observed_in_the_live_branch_alone() {
        for heap in [
            ObservedHeap::RegistrationDeliveryState,
            ObservedHeap::RegistrationInformation,
            ObservedHeap::SnapshotPreparation,
            ObservedHeap::WriteRecords,
        ] {
            assert_eq!(heap.presence(), RelationPresence::LiveOnly, "{heap:?}");
        }
        assert_eq!(ObservedHeap::AttributeCensus.presence(), RelationPresence::EveryBranch);
    }
}
