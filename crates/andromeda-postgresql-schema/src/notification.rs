//! The relations that record notification events and the deliveries they owe.
//!
//! `notification_events` is one immutable payload per (write, ordinal): the
//! state a subscriber must be told about, written once inside the transaction
//! that produced the change. `notification_deliveries` is the mutable work
//! queue built from those events — one row per matched subscription, carrying
//! the lease a worker claims it under and the retry bookkeeping.

use sea_query::Iden;

/// The `notification_events` table - one immutable payload per write ordinal.
#[derive(Iden, Clone, Copy)]
pub enum NotificationEvents {
    /// Table name: `notification_events`
    Table,
    /// Surrogate key referenced by every delivery that answers the event
    Id,
    /// Context-space partition that owns the event.
    ContextSpaceKey,
    /// Instant the event was written, the retention clock reads
    CreatedAt,
    /// Identity of the write that produced the event
    WriteId,
    /// Position of the event within its write, unique per write identity
    EventOrdinal,
    /// Migration-owned number of the trigger condition
    Trigger,
    /// Entity whose change the event records
    EntityId,
    /// Dictionary keys of the attributes the change touched
    ChangedAttributeNameIds,
    /// Dataset discriminator of an attribute-level event
    ChangedDatasetId,
    /// Entity state after the change
    EntitySnapshot,
    /// Entity state before the change
    PreviousSnapshot,
}

/// The `notification_deliveries` table - the mutable per-subscription queue.
#[derive(Iden, Clone, Copy)]
pub enum NotificationDeliveries {
    /// Table name: `notification_deliveries`
    Table,
    /// Delivery surrogate key
    Id,
    /// Context-space partition that owns the delivery
    ContextSpaceKey,
    /// Event the delivery answers
    EventId,
    /// External key of the subscription the delivery is owed to
    SubscriptionKey,
    /// Number of attempts made so far
    AttemptCount,
    /// Earliest instant the delivery may next be attempted
    NextAttemptAt,
    /// Instant the delivery was acknowledged as sent, `NULL` while pending
    DeliveredAt,
    /// Instant the worker gave the delivery up for good, `NULL` unless dead-lettered
    ExhaustedAt,
    /// Diagnostic recorded by the last failed attempt
    DeliveryError,
    /// Instant the current lease was taken
    ClaimedAt,
    /// Worker that holds the current lease
    ClaimedBy,
    /// Instant the current lease expires
    LeaseUntil,
}

#[cfg(test)]
mod tests {
    use crate::notification::{NotificationDeliveries, NotificationEvents};
    use sea_query::Iden;

    #[test]
    fn notification_events_table_and_columns() {
        assert_eq!(NotificationEvents::Table.to_string(), "notification_events");
        assert_eq!(NotificationEvents::Id.to_string(), "id");
        assert_eq!(NotificationEvents::CreatedAt.to_string(), "created_at");
        assert_eq!(NotificationEvents::WriteId.to_string(), "write_id");
        assert_eq!(NotificationEvents::EventOrdinal.to_string(), "event_ordinal");
        assert_eq!(NotificationEvents::Trigger.to_string(), "trigger");
        assert_eq!(NotificationEvents::EntityId.to_string(), "entity_id");
        assert_eq!(NotificationEvents::ChangedAttributeNameIds.to_string(), "changed_attribute_name_ids");
        assert_eq!(NotificationEvents::ChangedDatasetId.to_string(), "changed_dataset_id");
        assert_eq!(NotificationEvents::EntitySnapshot.to_string(), "entity_snapshot");
        assert_eq!(NotificationEvents::PreviousSnapshot.to_string(), "previous_snapshot");
    }

    #[test]
    fn notification_deliveries_table_and_columns() {
        assert_eq!(NotificationDeliveries::Table.to_string(), "notification_deliveries");
        assert_eq!(NotificationDeliveries::Id.to_string(), "id");
        assert_eq!(NotificationDeliveries::ContextSpaceKey.to_string(), "context_space_key");
        assert_eq!(NotificationDeliveries::EventId.to_string(), "event_id");
        assert_eq!(NotificationDeliveries::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(NotificationDeliveries::AttemptCount.to_string(), "attempt_count");
        assert_eq!(NotificationDeliveries::NextAttemptAt.to_string(), "next_attempt_at");
        assert_eq!(NotificationDeliveries::DeliveredAt.to_string(), "delivered_at");
        assert_eq!(NotificationDeliveries::ExhaustedAt.to_string(), "exhausted_at");
        assert_eq!(NotificationDeliveries::DeliveryError.to_string(), "delivery_error");
        assert_eq!(NotificationDeliveries::ClaimedAt.to_string(), "claimed_at");
        assert_eq!(NotificationDeliveries::ClaimedBy.to_string(), "claimed_by");
        assert_eq!(NotificationDeliveries::LeaseUntil.to_string(), "lease_until");
    }
}
