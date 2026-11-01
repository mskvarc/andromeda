//! The relations that record notification events and the deliveries they owe.
//!
//! `notification_events` is one immutable payload per (write, ordinal): the
//! state a subscriber must be told about, written once inside the transaction
//! that produced the change. `notification_deliveries` is the mutable work
//! queue built from those events — one row per matched subscription, carrying
//! the lease a worker claims it under and the retry bookkeeping.
//! `notification_delivery_holds` names, for each Subscription whose endpoint is
//! failing, the one delivery a claim may still take.

use sea_query::Iden;

/// The `notification_events` table - one immutable payload per write ordinal.
#[derive(Iden, Clone, Copy)]
pub enum NotificationEvents {
    /// Table name: `notification_events`
    Table,
    /// Surrogate key referenced by every delivery that answers the event
    EventKey,
    /// Instant the event was written, the retention clock reads
    CreatedAt,
    /// Store transaction timestamp of the commit that made the change
    CommittedAt,
    /// Identity of the write that produced the event
    WriteId,
    /// Position of the event within its write, unique per write identity
    EventOrdinal,
    /// Migration-owned number of the Entity's lifecycle transition
    Lifecycle,
    /// Entity whose change the event records
    EntityId,
    /// Migration-owned numbers of each changed Attribute instance's transition
    ChangeKinds,
    /// Dictionary keys of each changed Attribute instance's attribute
    ChangeAttributeNameKeys,
    /// Dataset discriminator of each changed Attribute instance, `NULL` for the default instance
    ChangeDatasetIds,
    /// Entity state after the change
    EntitySnapshot,
    /// Entity state before the change
    PreviousSnapshot,
    /// Clause 4.8 `createdAt` of an Entity a deletion removed
    EntityCreatedAt,
    /// Clause 4.8 `modifiedAt` of an Entity a deletion removed
    EntityModifiedAt,
    /// Clause 4.22 `expiresAt` of an Entity a deletion removed
    EntityExpiresAt,
}

/// The `notification_deliveries` table - the mutable per-subscription queue.
#[derive(Iden, Clone, Copy)]
pub enum NotificationDeliveries {
    /// Table name: `notification_deliveries`
    Table,
    /// Delivery surrogate key
    DeliveryKey,
    /// Event the delivery answers
    EventKey,
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
    /// Instant the delivery was set aside behind its Subscription's probe, `NULL` while it is claimable
    HeldAt,
    /// Diagnostic recorded by the last failed attempt (shared claimable-queue contract)
    LastError,
    /// Worker that holds the current lease
    ClaimedBy,
    /// Instant the current lease expires
    LeaseUntil,
}

/// The `notification_delivery_holds` table - the probe of each Subscription whose endpoint is failing.
#[derive(Iden, Clone, Copy)]
pub enum NotificationDeliveryHolds {
    /// Table name: `notification_delivery_holds`
    Table,
    /// The held Subscription, also the primary key
    SubscriptionKey,
    /// The probe: the one delivery of the Subscription a claim may take while the hold lasts
    DeliveryKey,
}

#[cfg(test)]
mod tests {
    use crate::notification::{NotificationDeliveries, NotificationDeliveryHolds, NotificationEvents};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn notification_events_table_and_columns() {
        assert_eq!(NotificationEvents::Table.to_string(), "notification_events");
        assert_eq!(NotificationEvents::EventKey.to_string(), "event_key");
        assert_eq!(NotificationEvents::CreatedAt.to_string(), "created_at");
        assert_eq!(NotificationEvents::CommittedAt.to_string(), "committed_at");
        assert_eq!(NotificationEvents::WriteId.to_string(), "write_id");
        assert_eq!(NotificationEvents::EventOrdinal.to_string(), "event_ordinal");
        assert_eq!(NotificationEvents::Lifecycle.to_string(), "lifecycle");
        assert_eq!(NotificationEvents::EntityId.to_string(), "entity_id");
        assert_eq!(NotificationEvents::ChangeKinds.to_string(), "change_kinds");
        assert_eq!(NotificationEvents::ChangeAttributeNameKeys.to_string(), "change_attribute_name_keys");
        assert_eq!(NotificationEvents::ChangeDatasetIds.to_string(), "change_dataset_ids");
        assert_eq!(NotificationEvents::EntitySnapshot.to_string(), "entity_snapshot");
        assert_eq!(NotificationEvents::PreviousSnapshot.to_string(), "previous_snapshot");
        assert_eq!(NotificationEvents::EntityCreatedAt.to_string(), "entity_created_at");
        assert_eq!(NotificationEvents::EntityModifiedAt.to_string(), "entity_modified_at");
        assert_eq!(NotificationEvents::EntityExpiresAt.to_string(), "entity_expires_at");
    }

    #[test]
    fn notification_deliveries_table_and_columns() {
        assert_eq!(NotificationDeliveries::Table.to_string(), "notification_deliveries");
        assert_eq!(NotificationDeliveries::DeliveryKey.to_string(), "delivery_key");
        assert_eq!(NotificationDeliveries::EventKey.to_string(), "event_key");
        assert_eq!(NotificationDeliveries::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(NotificationDeliveries::AttemptCount.to_string(), "attempt_count");
        assert_eq!(NotificationDeliveries::NextAttemptAt.to_string(), "next_attempt_at");
        assert_eq!(NotificationDeliveries::DeliveredAt.to_string(), "delivered_at");
        assert_eq!(NotificationDeliveries::ExhaustedAt.to_string(), "exhausted_at");
        assert_eq!(NotificationDeliveries::HeldAt.to_string(), "held_at");
        assert_eq!(NotificationDeliveries::LastError.to_string(), "last_error");
        assert_eq!(NotificationDeliveries::ClaimedBy.to_string(), "claimed_by");
        assert_eq!(NotificationDeliveries::LeaseUntil.to_string(), "lease_until");
    }

    #[test]
    fn notification_delivery_holds_table_and_columns() {
        assert_eq!(NotificationDeliveryHolds::Table.to_string(), "notification_delivery_holds");
        assert_eq!(NotificationDeliveryHolds::SubscriptionKey.to_string(), "subscription_key");
        assert_eq!(NotificationDeliveryHolds::DeliveryKey.to_string(), "delivery_key");
    }
}
