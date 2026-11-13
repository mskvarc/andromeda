//! The persisted relation one maintenance target row describes.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// A relation the PostgreSQL maintenance controller may act on.
    ///
    /// The labels are the relation names the controller inspects, which is how an
    /// operator recognises a target in the platform tables. A heap's planner
    /// statistics, refreshed by `ANALYZE` rather than by vacuum, are a target of
    /// their own, labelled with the heap's name and a `_statistics` suffix, and
    /// a history hypertable's columnstore is labelled with its name and a
    /// `_columnstore` suffix, and its kept rowstore chunks with its name and a
    /// `_rowstore` suffix. A heap repacked online to give its free space back
    /// is a target of its own too, labelled with the heap's name and a
    /// `_repack` suffix.
    MaintenanceTargetCode {
        frozen_prefix: 55,
        frozen_digest: 0x3fa9_82f9_158a_fe4d,
        codes: {
            /// The current attributes' grain primary key index.
            PkCaGrain = 1, "pk_ca_grain";
            /// The current attributes' number index.
            IdxCaValueNumber = 2, "idx_ca_value_number";
            /// The current attributes' string index.
            IdxCaValueString = 3, "idx_ca_value_string";
            /// The current attributes' trigram index.
            IdxCaValueStringTrgm = 4, "idx_ca_value_string_trgm";
            /// The current attributes' boolean index.
            IdxCaValueBoolean = 5, "idx_ca_value_boolean";
            /// The current attributes' date-time index.
            IdxCaValueDateTime = 6, "idx_ca_value_date_time";
            /// The current attributes' URI index.
            IdxCaValueUri = 7, "idx_ca_value_uri";
            /// The current attributes' geometry index.
            IdxCaValueGeometry = 8, "idx_ca_value_geometry";
            /// The current attributes' entity-geometry index.
            IdxCaEntityGeometry = 9, "idx_ca_entity_geometry";
            /// The current attributes' sub-attribute equality key index.
            IdxCaSubEqKeys = 10, "idx_ca_sub_eq_keys";
            /// The current attributes' heap.
            CurrentAttributes = 11, "current_attributes";
            /// The current entities' heap, whose version token every write updates.
            Entities = 12, "entities";
            /// The subscription delivery-state heap.
            SubscriptionDeliveryState = 13, "subscription_delivery_state";
            /// The current attributes' JSON object containment index.
            IdxCaValueObject = 14, "idx_ca_value_object";
            /// The current attributes' long-string trigram index.
            IdxCaValueLongTextTrgm = 15, "idx_ca_value_long_text_trgm";
            /// The notification events queue heap.
            NotificationEvents = 16, "notification_events";
            /// The notification deliveries queue heap.
            NotificationDeliveries = 17, "notification_deliveries";
            /// The pending side effects queue heap.
            PendingSideEffects = 18, "pending_side_effects";
            /// The temporal append intents queue heap.
            TemporalAppendIntents = 19, "temporal_append_intents";
            /// The snapshot notification outbox queue heap.
            SnapshotNotificationOutbox = 20, "snapshot_notification_outbox";
            /// The entity search projection's heap.
            EntitySearch = 21, "entity_search";
            /// The current attributes' planner statistics.
            CurrentAttributesStatistics = 22, "current_attributes_statistics";
            /// The current entities' planner statistics.
            EntitiesStatistics = 23, "entities_statistics";
            /// The subscription delivery-state planner statistics.
            SubscriptionDeliveryStateStatistics = 24, "subscription_delivery_state_statistics";
            /// The notification events queue's planner statistics.
            NotificationEventsStatistics = 25, "notification_events_statistics";
            /// The notification deliveries queue's planner statistics.
            NotificationDeliveriesStatistics = 26, "notification_deliveries_statistics";
            /// The pending side effects queue's planner statistics.
            PendingSideEffectsStatistics = 27, "pending_side_effects_statistics";
            /// The temporal append intents queue's planner statistics.
            TemporalAppendIntentsStatistics = 28, "temporal_append_intents_statistics";
            /// The snapshot notification outbox queue's planner statistics.
            SnapshotNotificationOutboxStatistics = 29, "snapshot_notification_outbox_statistics";
            /// The entity search projection's planner statistics.
            EntitySearchStatistics = 30, "entity_search_statistics";
            /// The notification deliveries' claim index.
            IdxNotificationDeliveriesClaim = 31, "idx_notification_deliveries_claim";
            /// The notification deliveries' one-per-event-and-subscription constraint index.
            UqNotificationDeliveriesEventSubscription = 32, "uq_notification_deliveries_event_subscription";
            /// The notification deliveries' subscription index.
            IdxNotificationDeliveriesSubscription = 33, "idx_notification_deliveries_subscription";
            /// The notification deliveries' primary key index.
            PkNotificationDeliveries = 34, "pk_notification_deliveries";
            /// The attribute history hypertable's partially converted chunks.
            AttributeHistoryColumnstore = 35, "attribute_history_columnstore";
            /// The entities' identifier trigram index.
            IdxEntitiesIdTrgm = 36, "idx_entities_id_trgm";
            /// The temporal attribute stream catalogue's heap.
            TemporalAttributeStreams = 37, "temporal_attribute_streams";
            /// The temporal attribute stream catalogue's planner statistics.
            TemporalAttributeStreamsStatistics = 38, "temporal_attribute_streams_statistics";
            /// The stream catalogue's primary key index.
            PkTemporalAttributeStreams = 39, "pk_temporal_attribute_streams";
            /// The stream catalogue's grain constraint index.
            UqTemporalAttributeStreamsGrain = 40, "uq_temporal_attribute_streams_grain";
            /// The stream catalogue's attribute-led index.
            IdxTemporalAttributeStreamsAttributeNameKeyEntityKey = 41, "idx_temporal_attribute_streams_attribute_name_key_entity_key";
            /// The stream catalogue's value extent index.
            IdxTemporalAttributeStreamsValueExtent = 42, "idx_temporal_attribute_streams_value_extent";
            /// The stream catalogue's shape mask index.
            IdxTemporalAttributeStreamsAttributeNameKeyShapeMask = 43, "idx_temporal_attribute_streams_attribute_name_key_shape_mask";
            /// The attribute history hypertable's kept rowstore chunks.
            AttributeHistoryRowstore = 44, "attribute_history_rowstore";
            /// The notification deliveries' per-subscription claim index.
            IdxNotificationDeliveriesSubscriptionClaim = 45, "idx_notification_deliveries_subscription_claim";
            /// The current attributes' heap, repacked online to give its free space back.
            CurrentAttributesRepack = 46, "current_attributes_repack";
            /// The current entities' heap, repacked online to give its free space back.
            EntitiesRepack = 47, "entities_repack";
            /// The subscription delivery-state heap, repacked online to give its free space back.
            SubscriptionDeliveryStateRepack = 48, "subscription_delivery_state_repack";
            /// The notification events queue heap, repacked online to give its free space back.
            NotificationEventsRepack = 49, "notification_events_repack";
            /// The notification deliveries queue heap, repacked online to give its free space back.
            NotificationDeliveriesRepack = 50, "notification_deliveries_repack";
            /// The pending side effects queue heap, repacked online to give its free space back.
            PendingSideEffectsRepack = 51, "pending_side_effects_repack";
            /// The temporal append intents queue heap, repacked online to give its free space back.
            TemporalAppendIntentsRepack = 52, "temporal_append_intents_repack";
            /// The snapshot notification outbox queue heap, repacked online to give its free space back.
            SnapshotNotificationOutboxRepack = 53, "snapshot_notification_outbox_repack";
            /// The entity search projection's heap, repacked online to give its free space back.
            EntitySearchRepack = 54, "entity_search_repack";
            /// The temporal attribute stream catalogue's heap, repacked online to give its free space back.
            TemporalAttributeStreamsRepack = 55, "temporal_attribute_streams_repack";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_target::MaintenanceTargetCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceTargetCode::ALL.len(), 55);
        assert_eq!(MaintenanceTargetCode::PkCaGrain.code(), 1);
        assert_eq!(MaintenanceTargetCode::IdxCaSubEqKeys.code(), 10);
        assert_eq!(MaintenanceTargetCode::SubscriptionDeliveryState.code(), 13);
        assert_eq!(MaintenanceTargetCode::IdxCaValueObject.code(), 14);
        assert_eq!(MaintenanceTargetCode::IdxCaValueLongTextTrgm.code(), 15);
        assert_eq!(MaintenanceTargetCode::NotificationEvents.code(), 16);
        assert_eq!(MaintenanceTargetCode::SnapshotNotificationOutbox.code(), 20);
        assert_eq!(MaintenanceTargetCode::EntitySearch.code(), 21);
        assert_eq!(MaintenanceTargetCode::CurrentAttributesStatistics.code(), 22);
        assert_eq!(MaintenanceTargetCode::EntitySearchStatistics.code(), 30);
        assert_eq!(MaintenanceTargetCode::IdxNotificationDeliveriesClaim.code(), 31);
        assert_eq!(MaintenanceTargetCode::PkNotificationDeliveries.code(), 34);
        assert_eq!(MaintenanceTargetCode::AttributeHistoryColumnstore.code(), 35);
        assert_eq!(MaintenanceTargetCode::IdxEntitiesIdTrgm.code(), 36);
        assert_eq!(MaintenanceTargetCode::TemporalAttributeStreams.code(), 37);
        assert_eq!(MaintenanceTargetCode::IdxTemporalAttributeStreamsValueExtent.code(), 42);
        assert_eq!(MaintenanceTargetCode::AttributeHistoryRowstore.code(), 44);
        assert_eq!(MaintenanceTargetCode::IdxNotificationDeliveriesSubscriptionClaim.code(), 45);
        assert_eq!(MaintenanceTargetCode::CurrentAttributesRepack.code(), 46);
        assert_eq!(MaintenanceTargetCode::TemporalAttributeStreamsRepack.code(), 55);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceTargetCode::try_from(0).is_err());
        assert!(MaintenanceTargetCode::try_from(56).is_err());
    }
}
