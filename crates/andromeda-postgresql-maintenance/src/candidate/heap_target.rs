//! The heaps eligible for catch-up vacuum and `ANALYZE`, and the branches whose schema holds each.

use crate::branch::Branch;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities, EntitySearch},
    notification::{NotificationDeliveries, NotificationEvents},
    queue::{PendingSideEffects, TemporalAppendIntents},
    snapshot::SnapshotNotificationOutbox,
    subscriptions::SubscriptionDeliveryState,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceStatement, analyze, vacuum_non_truncating};
use sea_query::Iden;
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
}

impl HeapTarget {
    /// Every heap relation eligible for cheap sampling, catch-up vacuum and
    /// `ANALYZE`.
    pub const ALL: [HeapTarget; 9] = [
        HeapTarget::CurrentAttributes,
        HeapTarget::Entities,
        HeapTarget::SubscriptionDeliveryState,
        HeapTarget::NotificationEvents,
        HeapTarget::NotificationDeliveries,
        HeapTarget::PendingSideEffects,
        HeapTarget::TemporalAppendIntents,
        HeapTarget::SnapshotNotificationOutbox,
        HeapTarget::EntitySearch,
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
        }
    }

    /// A fixed non-truncating, non-parallel ordinary vacuum statement in
    /// `branch`'s schema.
    pub(crate) fn vacuum_statement(self, branch: Branch) -> MaintenanceStatement {
        self.statement(branch, vacuum_non_truncating)
    }

    /// A fixed `ANALYZE` statement in `branch`'s schema.
    pub(crate) fn analyze_statement(self, branch: Branch) -> MaintenanceStatement {
        self.statement(branch, analyze)
    }

    /// `build` applied to `branch`'s schema and this heap's table identifier.
    fn statement(self, branch: Branch, build: fn(&dyn Iden, &dyn Iden) -> MaintenanceStatement) -> MaintenanceStatement {
        let schema = branch.schema();
        match self {
            HeapTarget::CurrentAttributes => build(&schema, &CurrentAttributes::Table),
            HeapTarget::Entities => build(&schema, &Entities::Table),
            HeapTarget::SubscriptionDeliveryState => build(&schema, &SubscriptionDeliveryState::Table),
            HeapTarget::NotificationEvents => build(&schema, &NotificationEvents::Table),
            HeapTarget::NotificationDeliveries => build(&schema, &NotificationDeliveries::Table),
            HeapTarget::PendingSideEffects => build(&schema, &PendingSideEffects::Table),
            HeapTarget::TemporalAppendIntents => build(&schema, &TemporalAppendIntents::Table),
            HeapTarget::SnapshotNotificationOutbox => build(&schema, &SnapshotNotificationOutbox::Table),
            HeapTarget::EntitySearch => build(&schema, &EntitySearch::Table),
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
            | HeapTarget::EntitySearch => RelationPresence::EveryBranch,
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
    use pretty_assertions::assert_eq;

    #[test]
    fn every_heap_command_disables_truncation_and_parallel_workers() {
        for target in HeapTarget::ALL {
            let statement = target.vacuum_statement(Branch::Live);
            let command = statement.as_sql();
            assert!(command.starts_with("VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\"."), "{command}");
        }
    }

    #[test]
    fn analyze_statement_is_fixed_per_table() {
        for heap in HeapTarget::ALL {
            assert_eq!(
                heap.analyze_statement(Branch::Live).as_sql(),
                format!("ANALYZE \"public\".\"{}\"", heap.relation_name()),
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
            let statement = heap.vacuum_statement(Branch::Live);
            assert_eq!(
                statement.as_sql(),
                format!("VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\".\"{table}\""),
                "{heap}"
            );
        }
    }
}
