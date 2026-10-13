//! The composite type one change a respond-async job owes waits in.

use sea_query::Iden;

/// The fields of the `pending_change_event` composite type, in the type's
/// declaration order.
///
/// The fields a `notification_events` row also holds carry its column names
/// in its column order; the others are what a notification needs before any
/// event row exists.
#[derive(Iden, Clone, Copy)]
pub enum PendingChangeEvent {
    /// Type name: `pending_change_event`
    #[iden = "pending_change_event"]
    Type,
    /// What the write did to the Entity: 1 created, 2 modified, 3 deleted
    Lifecycle,
    /// The Entity changed
    EntityId,
    /// `entity_type_names` keys of the Entity's types, in written order
    EntityTypeNameKeys,
    /// The transition of each changed Attribute instance: 1 created, 2 updated, 3 deleted
    ChangeKinds,
    /// `attribute_names` key of each changed instance, position for position
    ChangeAttributeNameKeys,
    /// `datasetId` of each changed instance, NULL for the default instance
    ChangeDatasetIds,
    /// Instant the change occurred
    OccurredAt,
    /// Where the change came from: 1 local, 2 forwarded from a context source
    Origin,
    /// The Entity's state after the change, or the state a deletion removed
    EntitySnapshot,
    /// The Entity's state before a change that left it in place
    PreviousSnapshot,
    /// Clause 4.8 `createdAt` of an Entity a deletion removed
    EntityCreatedAt,
    /// Clause 4.8 `modifiedAt` of an Entity a deletion removed
    EntityModifiedAt,
    /// Clause 4.22 `expiresAt` of an Entity a deletion removed
    EntityExpiresAt,
}

#[cfg(test)]
mod tests {
    use crate::{notification::NotificationEvents, pending_change_event::PendingChangeEvent};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn the_event_fields_follow_the_notification_event_columns() {
        let shared = [
            PendingChangeEvent::Lifecycle,
            PendingChangeEvent::EntityId,
            PendingChangeEvent::ChangeKinds,
            PendingChangeEvent::ChangeAttributeNameKeys,
            PendingChangeEvent::ChangeDatasetIds,
            PendingChangeEvent::EntitySnapshot,
            PendingChangeEvent::PreviousSnapshot,
            PendingChangeEvent::EntityCreatedAt,
            PendingChangeEvent::EntityModifiedAt,
            PendingChangeEvent::EntityExpiresAt,
        ]
        .map(|field| field.to_string());
        let columns = [
            NotificationEvents::Lifecycle,
            NotificationEvents::EntityId,
            NotificationEvents::ChangeKinds,
            NotificationEvents::ChangeAttributeNameKeys,
            NotificationEvents::ChangeDatasetIds,
            NotificationEvents::EntitySnapshot,
            NotificationEvents::PreviousSnapshot,
            NotificationEvents::EntityCreatedAt,
            NotificationEvents::EntityModifiedAt,
            NotificationEvents::EntityExpiresAt,
        ]
        .map(|column| column.to_string());
        assert_eq!(PendingChangeEvent::Type.to_string(), "pending_change_event");
        assert_eq!(shared, columns);
    }
}
