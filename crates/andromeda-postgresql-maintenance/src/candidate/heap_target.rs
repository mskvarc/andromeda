//! The heaps eligible for catch-up vacuum and `ANALYZE`, and the branches whose schema holds each.

use crate::branch::Branch;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities, EntitySearch},
    notification::{NotificationDeliveries, NotificationEvents},
    queue::{PendingSideEffects, TemporalAppendIntents},
    snapshot::SnapshotNotificationOutbox,
    subscriptions::SubscriptionDeliveryState,
    temporal::TemporalAttributeStreams,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceCommand, analyze, vacuum_non_truncating};
use sea_query::{DynIden, IntoIden};
use strum::{Display, EnumString, IntoStaticStr};

/// A heap relation eligible for catch-up ordinary vacuum and for `ANALYZE`.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HeapTarget {
    /// Current attribute instances.
    CurrentAttributes,
    /// Current entities, whose version token every write updates.
    Entities,
    /// Frequently updated subscription delivery counters.
    SubscriptionDeliveryState,
    /// Notification events awaiting fan-out to their deliveries.
    NotificationEvents,
    /// Notification deliveries, leased by every claim.
    NotificationDeliveries,
    /// Write side effects awaiting their retry.
    PendingSideEffects,
    /// Temporal history appends awaiting their retry.
    TemporalAppendIntents,
    /// Snapshot notifications awaiting delivery, leased by every claim.
    SnapshotNotificationOutbox,
    /// The entity search projection, rewritten with its entity.
    EntitySearch,
    /// The temporal stream catalogue, whose value extent and shape mask a
    /// history write grows.
    TemporalAttributeStreams,
}

impl HeapTarget {
    /// Every heap relation eligible for cheap sampling, catch-up vacuum and
    /// `ANALYZE`.
    pub const ALL: [HeapTarget; 10] = [
        HeapTarget::CurrentAttributes,
        HeapTarget::Entities,
        HeapTarget::SubscriptionDeliveryState,
        HeapTarget::NotificationEvents,
        HeapTarget::NotificationDeliveries,
        HeapTarget::PendingSideEffects,
        HeapTarget::TemporalAppendIntents,
        HeapTarget::SnapshotNotificationOutbox,
        HeapTarget::EntitySearch,
        HeapTarget::TemporalAttributeStreams,
    ];

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            HeapTarget::CurrentAttributes => "current_attributes",
            HeapTarget::Entities => "entities",
            HeapTarget::SubscriptionDeliveryState => "subscription_delivery_state",
            HeapTarget::NotificationEvents => "notification_events",
            HeapTarget::NotificationDeliveries => "notification_deliveries",
            HeapTarget::PendingSideEffects => "pending_side_effects",
            HeapTarget::TemporalAppendIntents => "temporal_append_intents",
            HeapTarget::SnapshotNotificationOutbox => "snapshot_notification_outbox",
            HeapTarget::EntitySearch => "entity_search",
            HeapTarget::TemporalAttributeStreams => "temporal_attribute_streams",
        }
    }

    /// A fixed non-truncating, non-parallel ordinary vacuum statement in
    /// `branch`'s schema.
    pub(crate) fn vacuum_statement(self, branch: Branch) -> MaintenanceCommand {
        vacuum_non_truncating(branch.schema(), self.table())
    }

    /// A fixed `ANALYZE` statement in `branch`'s schema.
    pub(crate) fn analyze_statement(self, branch: Branch) -> MaintenanceCommand {
        analyze(branch.schema(), self.table())
    }

    /// This heap's table identifier.
    fn table(self) -> DynIden {
        match self {
            HeapTarget::CurrentAttributes => CurrentAttributes::Table.into_iden(),
            HeapTarget::Entities => Entities::Table.into_iden(),
            HeapTarget::SubscriptionDeliveryState => SubscriptionDeliveryState::Table.into_iden(),
            HeapTarget::NotificationEvents => NotificationEvents::Table.into_iden(),
            HeapTarget::NotificationDeliveries => NotificationDeliveries::Table.into_iden(),
            HeapTarget::PendingSideEffects => PendingSideEffects::Table.into_iden(),
            HeapTarget::TemporalAppendIntents => TemporalAppendIntents::Table.into_iden(),
            HeapTarget::SnapshotNotificationOutbox => SnapshotNotificationOutbox::Table.into_iden(),
            HeapTarget::EntitySearch => EntitySearch::Table.into_iden(),
            HeapTarget::TemporalAttributeStreams => TemporalAttributeStreams::Table.into_iden(),
        }
    }

    /// The context branches whose schema holds the heap.
    #[must_use]
    pub const fn presence(self) -> RelationPresence {
        match self {
            HeapTarget::CurrentAttributes
            | HeapTarget::Entities
            | HeapTarget::SubscriptionDeliveryState
            | HeapTarget::NotificationEvents
            | HeapTarget::NotificationDeliveries
            | HeapTarget::EntitySearch
            | HeapTarget::TemporalAttributeStreams => RelationPresence::EveryBranch,
            HeapTarget::PendingSideEffects | HeapTarget::TemporalAppendIntents | HeapTarget::SnapshotNotificationOutbox => RelationPresence::LiveOnly,
        }
    }
}

/// Which context branches hold a relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelationPresence {
    /// Created by the branch migrations, so the live schema and every
    /// Snapshot branch schema hold it.
    EveryBranch,
    /// Created by the tenant migrations, so only the live schema holds it.
    LiveOnly,
}

#[cfg(test)]
mod tests {
    use crate::{branch::Branch, candidate::heap_target::HeapTarget};
    use andromeda_postgresql_seaquery::maintenance::{analyze, vacuum_non_truncating};
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn every_heap_command_disables_truncation_and_parallel_workers() {
        for target in HeapTarget::ALL {
            assert_eq!(
                target.vacuum_statement(Branch::Live),
                vacuum_non_truncating(Alias::new("public"), Alias::new(target.relation_name())),
                "{target}"
            );
        }
    }

    #[test]
    fn analyze_statement_is_fixed_per_table() {
        for heap in HeapTarget::ALL {
            assert_eq!(
                heap.analyze_statement(Branch::Live),
                analyze(Alias::new("public"), Alias::new(heap.relation_name())),
                "{heap}"
            );
        }
    }

    #[test]
    fn the_heap_targets_are_the_hot_heaps_and_the_queues() {
        assert_eq!(
            HeapTarget::ALL.map(HeapTarget::relation_name),
            [
                "current_attributes",
                "entities",
                "subscription_delivery_state",
                "notification_events",
                "notification_deliveries",
                "pending_side_effects",
                "temporal_append_intents",
                "snapshot_notification_outbox",
                "entity_search",
                "temporal_attribute_streams",
            ]
        );
    }

    #[test]
    fn every_queue_heap_has_a_fixed_non_truncating_vacuum() {
        for (heap, table) in [
            (HeapTarget::NotificationEvents, "notification_events"),
            (HeapTarget::NotificationDeliveries, "notification_deliveries"),
            (HeapTarget::PendingSideEffects, "pending_side_effects"),
            (HeapTarget::TemporalAppendIntents, "temporal_append_intents"),
            (HeapTarget::SnapshotNotificationOutbox, "snapshot_notification_outbox"),
        ] {
            assert_eq!(
                heap.vacuum_statement(Branch::Live),
                vacuum_non_truncating(Alias::new("public"), Alias::new(table)),
                "{heap}"
            );
        }
    }
}
