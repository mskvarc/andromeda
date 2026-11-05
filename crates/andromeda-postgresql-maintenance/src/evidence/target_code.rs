//! The number a maintenance target is stored as.

use crate::candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;

/// The registry member the platform database stores for a target.
pub(crate) const fn stored_target(target: MaintenanceTarget) -> MaintenanceTargetCode {
    match target {
        MaintenanceTarget::Index(IndexTarget::PkCaGrain) => MaintenanceTargetCode::PkCaGrain,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueNumber) => MaintenanceTargetCode::IdxCaValueNumber,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueString) => MaintenanceTargetCode::IdxCaValueString,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm) => MaintenanceTargetCode::IdxCaValueStringTrgm,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueBoolean) => MaintenanceTargetCode::IdxCaValueBoolean,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueDateTime) => MaintenanceTargetCode::IdxCaValueDateTime,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueUri) => MaintenanceTargetCode::IdxCaValueUri,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueGeometry) => MaintenanceTargetCode::IdxCaValueGeometry,
        MaintenanceTarget::Index(IndexTarget::IdxCaEntityGeometry) => MaintenanceTargetCode::IdxCaEntityGeometry,
        MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys) => MaintenanceTargetCode::IdxCaSubEqKeys,
        MaintenanceTarget::Heap(HeapTarget::CurrentAttributes) => MaintenanceTargetCode::CurrentAttributes,
        MaintenanceTarget::Heap(HeapTarget::Entities) => MaintenanceTargetCode::Entities,
        MaintenanceTarget::Heap(HeapTarget::SubscriptionDeliveryState) => MaintenanceTargetCode::SubscriptionDeliveryState,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueObject) => MaintenanceTargetCode::IdxCaValueObject,
        MaintenanceTarget::Index(IndexTarget::IdxCaValueLongTextTrgm) => MaintenanceTargetCode::IdxCaValueLongTextTrgm,
        MaintenanceTarget::Heap(HeapTarget::NotificationEvents) => MaintenanceTargetCode::NotificationEvents,
        MaintenanceTarget::Heap(HeapTarget::NotificationDeliveries) => MaintenanceTargetCode::NotificationDeliveries,
        MaintenanceTarget::Heap(HeapTarget::PendingSideEffects) => MaintenanceTargetCode::PendingSideEffects,
        MaintenanceTarget::Heap(HeapTarget::TemporalAppendIntents) => MaintenanceTargetCode::TemporalAppendIntents,
        MaintenanceTarget::Heap(HeapTarget::SnapshotNotificationOutbox) => MaintenanceTargetCode::SnapshotNotificationOutbox,
        MaintenanceTarget::Heap(HeapTarget::EntitySearch) => MaintenanceTargetCode::EntitySearch,
        MaintenanceTarget::Statistics(HeapTarget::CurrentAttributes) => MaintenanceTargetCode::CurrentAttributesStatistics,
        MaintenanceTarget::Statistics(HeapTarget::Entities) => MaintenanceTargetCode::EntitiesStatistics,
        MaintenanceTarget::Statistics(HeapTarget::SubscriptionDeliveryState) => MaintenanceTargetCode::SubscriptionDeliveryStateStatistics,
        MaintenanceTarget::Statistics(HeapTarget::NotificationEvents) => MaintenanceTargetCode::NotificationEventsStatistics,
        MaintenanceTarget::Statistics(HeapTarget::NotificationDeliveries) => MaintenanceTargetCode::NotificationDeliveriesStatistics,
        MaintenanceTarget::Statistics(HeapTarget::PendingSideEffects) => MaintenanceTargetCode::PendingSideEffectsStatistics,
        MaintenanceTarget::Statistics(HeapTarget::TemporalAppendIntents) => MaintenanceTargetCode::TemporalAppendIntentsStatistics,
        MaintenanceTarget::Statistics(HeapTarget::SnapshotNotificationOutbox) => MaintenanceTargetCode::SnapshotNotificationOutboxStatistics,
        MaintenanceTarget::Statistics(HeapTarget::EntitySearch) => MaintenanceTargetCode::EntitySearchStatistics,
        MaintenanceTarget::Index(IndexTarget::IdxNotificationDeliveriesClaim) => MaintenanceTargetCode::IdxNotificationDeliveriesClaim,
        MaintenanceTarget::Index(IndexTarget::UqNotificationDeliveriesEventSubscription) => MaintenanceTargetCode::UqNotificationDeliveriesEventSubscription,
        MaintenanceTarget::Index(IndexTarget::IdxNotificationDeliveriesSubscription) => MaintenanceTargetCode::IdxNotificationDeliveriesSubscription,
        MaintenanceTarget::Index(IndexTarget::IdxNotificationDeliveriesSubscriptionClaim) => MaintenanceTargetCode::IdxNotificationDeliveriesSubscriptionClaim,
        MaintenanceTarget::Index(IndexTarget::PkNotificationDeliveries) => MaintenanceTargetCode::PkNotificationDeliveries,
        MaintenanceTarget::Index(IndexTarget::IdxEntitiesIdTrgm) => MaintenanceTargetCode::IdxEntitiesIdTrgm,
        MaintenanceTarget::Index(IndexTarget::PkTemporalAttributeStreams) => MaintenanceTargetCode::PkTemporalAttributeStreams,
        MaintenanceTarget::Index(IndexTarget::UqTemporalAttributeStreamsGrain) => MaintenanceTargetCode::UqTemporalAttributeStreamsGrain,
        MaintenanceTarget::Index(IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey) => {
            MaintenanceTargetCode::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey
        }
        MaintenanceTarget::Index(IndexTarget::IdxTemporalAttributeStreamsValueExtent) => MaintenanceTargetCode::IdxTemporalAttributeStreamsValueExtent,
        MaintenanceTarget::Index(IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask) => {
            MaintenanceTargetCode::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask
        }
        MaintenanceTarget::Heap(HeapTarget::TemporalAttributeStreams) => MaintenanceTargetCode::TemporalAttributeStreams,
        MaintenanceTarget::Statistics(HeapTarget::TemporalAttributeStreams) => MaintenanceTargetCode::TemporalAttributeStreamsStatistics,
        MaintenanceTarget::Rowstore(HistoryTable::AttributeHistory) => MaintenanceTargetCode::AttributeHistoryRowstore,
        MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory) => MaintenanceTargetCode::AttributeHistoryColumnstore,
        MaintenanceTarget::Repack(HeapTarget::CurrentAttributes) => MaintenanceTargetCode::CurrentAttributesRepack,
        MaintenanceTarget::Repack(HeapTarget::Entities) => MaintenanceTargetCode::EntitiesRepack,
        MaintenanceTarget::Repack(HeapTarget::SubscriptionDeliveryState) => MaintenanceTargetCode::SubscriptionDeliveryStateRepack,
        MaintenanceTarget::Repack(HeapTarget::NotificationEvents) => MaintenanceTargetCode::NotificationEventsRepack,
        MaintenanceTarget::Repack(HeapTarget::NotificationDeliveries) => MaintenanceTargetCode::NotificationDeliveriesRepack,
        MaintenanceTarget::Repack(HeapTarget::PendingSideEffects) => MaintenanceTargetCode::PendingSideEffectsRepack,
        MaintenanceTarget::Repack(HeapTarget::TemporalAppendIntents) => MaintenanceTargetCode::TemporalAppendIntentsRepack,
        MaintenanceTarget::Repack(HeapTarget::SnapshotNotificationOutbox) => MaintenanceTargetCode::SnapshotNotificationOutboxRepack,
        MaintenanceTarget::Repack(HeapTarget::EntitySearch) => MaintenanceTargetCode::EntitySearchRepack,
        MaintenanceTarget::Repack(HeapTarget::TemporalAttributeStreams) => MaintenanceTargetCode::TemporalAttributeStreamsRepack,
    }
}

/// The target a stored registry member names.
pub(crate) const fn target_of(code: MaintenanceTargetCode) -> MaintenanceTarget {
    match code {
        MaintenanceTargetCode::PkCaGrain => MaintenanceTarget::Index(IndexTarget::PkCaGrain),
        MaintenanceTargetCode::IdxCaValueNumber => MaintenanceTarget::Index(IndexTarget::IdxCaValueNumber),
        MaintenanceTargetCode::IdxCaValueString => MaintenanceTarget::Index(IndexTarget::IdxCaValueString),
        MaintenanceTargetCode::IdxCaValueStringTrgm => MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm),
        MaintenanceTargetCode::IdxCaValueBoolean => MaintenanceTarget::Index(IndexTarget::IdxCaValueBoolean),
        MaintenanceTargetCode::IdxCaValueDateTime => MaintenanceTarget::Index(IndexTarget::IdxCaValueDateTime),
        MaintenanceTargetCode::IdxCaValueUri => MaintenanceTarget::Index(IndexTarget::IdxCaValueUri),
        MaintenanceTargetCode::IdxCaValueGeometry => MaintenanceTarget::Index(IndexTarget::IdxCaValueGeometry),
        MaintenanceTargetCode::IdxCaEntityGeometry => MaintenanceTarget::Index(IndexTarget::IdxCaEntityGeometry),
        MaintenanceTargetCode::IdxCaSubEqKeys => MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys),
        MaintenanceTargetCode::CurrentAttributes => MaintenanceTarget::Heap(HeapTarget::CurrentAttributes),
        MaintenanceTargetCode::Entities => MaintenanceTarget::Heap(HeapTarget::Entities),
        MaintenanceTargetCode::SubscriptionDeliveryState => MaintenanceTarget::Heap(HeapTarget::SubscriptionDeliveryState),
        MaintenanceTargetCode::IdxCaValueObject => MaintenanceTarget::Index(IndexTarget::IdxCaValueObject),
        MaintenanceTargetCode::IdxCaValueLongTextTrgm => MaintenanceTarget::Index(IndexTarget::IdxCaValueLongTextTrgm),
        MaintenanceTargetCode::NotificationEvents => MaintenanceTarget::Heap(HeapTarget::NotificationEvents),
        MaintenanceTargetCode::NotificationDeliveries => MaintenanceTarget::Heap(HeapTarget::NotificationDeliveries),
        MaintenanceTargetCode::PendingSideEffects => MaintenanceTarget::Heap(HeapTarget::PendingSideEffects),
        MaintenanceTargetCode::TemporalAppendIntents => MaintenanceTarget::Heap(HeapTarget::TemporalAppendIntents),
        MaintenanceTargetCode::SnapshotNotificationOutbox => MaintenanceTarget::Heap(HeapTarget::SnapshotNotificationOutbox),
        MaintenanceTargetCode::EntitySearch => MaintenanceTarget::Heap(HeapTarget::EntitySearch),
        MaintenanceTargetCode::CurrentAttributesStatistics => MaintenanceTarget::Statistics(HeapTarget::CurrentAttributes),
        MaintenanceTargetCode::EntitiesStatistics => MaintenanceTarget::Statistics(HeapTarget::Entities),
        MaintenanceTargetCode::SubscriptionDeliveryStateStatistics => MaintenanceTarget::Statistics(HeapTarget::SubscriptionDeliveryState),
        MaintenanceTargetCode::NotificationEventsStatistics => MaintenanceTarget::Statistics(HeapTarget::NotificationEvents),
        MaintenanceTargetCode::NotificationDeliveriesStatistics => MaintenanceTarget::Statistics(HeapTarget::NotificationDeliveries),
        MaintenanceTargetCode::PendingSideEffectsStatistics => MaintenanceTarget::Statistics(HeapTarget::PendingSideEffects),
        MaintenanceTargetCode::TemporalAppendIntentsStatistics => MaintenanceTarget::Statistics(HeapTarget::TemporalAppendIntents),
        MaintenanceTargetCode::SnapshotNotificationOutboxStatistics => MaintenanceTarget::Statistics(HeapTarget::SnapshotNotificationOutbox),
        MaintenanceTargetCode::EntitySearchStatistics => MaintenanceTarget::Statistics(HeapTarget::EntitySearch),
        MaintenanceTargetCode::IdxNotificationDeliveriesClaim => MaintenanceTarget::Index(IndexTarget::IdxNotificationDeliveriesClaim),
        MaintenanceTargetCode::UqNotificationDeliveriesEventSubscription => MaintenanceTarget::Index(IndexTarget::UqNotificationDeliveriesEventSubscription),
        MaintenanceTargetCode::IdxNotificationDeliveriesSubscription => MaintenanceTarget::Index(IndexTarget::IdxNotificationDeliveriesSubscription),
        MaintenanceTargetCode::IdxNotificationDeliveriesSubscriptionClaim => MaintenanceTarget::Index(IndexTarget::IdxNotificationDeliveriesSubscriptionClaim),
        MaintenanceTargetCode::PkNotificationDeliveries => MaintenanceTarget::Index(IndexTarget::PkNotificationDeliveries),
        MaintenanceTargetCode::IdxEntitiesIdTrgm => MaintenanceTarget::Index(IndexTarget::IdxEntitiesIdTrgm),
        MaintenanceTargetCode::PkTemporalAttributeStreams => MaintenanceTarget::Index(IndexTarget::PkTemporalAttributeStreams),
        MaintenanceTargetCode::UqTemporalAttributeStreamsGrain => MaintenanceTarget::Index(IndexTarget::UqTemporalAttributeStreamsGrain),
        MaintenanceTargetCode::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey => {
            MaintenanceTarget::Index(IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey)
        }
        MaintenanceTargetCode::IdxTemporalAttributeStreamsValueExtent => MaintenanceTarget::Index(IndexTarget::IdxTemporalAttributeStreamsValueExtent),
        MaintenanceTargetCode::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask => {
            MaintenanceTarget::Index(IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask)
        }
        MaintenanceTargetCode::TemporalAttributeStreams => MaintenanceTarget::Heap(HeapTarget::TemporalAttributeStreams),
        MaintenanceTargetCode::TemporalAttributeStreamsStatistics => MaintenanceTarget::Statistics(HeapTarget::TemporalAttributeStreams),
        MaintenanceTargetCode::AttributeHistoryRowstore => MaintenanceTarget::Rowstore(HistoryTable::AttributeHistory),
        MaintenanceTargetCode::AttributeHistoryColumnstore => MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory),
        MaintenanceTargetCode::CurrentAttributesRepack => MaintenanceTarget::Repack(HeapTarget::CurrentAttributes),
        MaintenanceTargetCode::EntitiesRepack => MaintenanceTarget::Repack(HeapTarget::Entities),
        MaintenanceTargetCode::SubscriptionDeliveryStateRepack => MaintenanceTarget::Repack(HeapTarget::SubscriptionDeliveryState),
        MaintenanceTargetCode::NotificationEventsRepack => MaintenanceTarget::Repack(HeapTarget::NotificationEvents),
        MaintenanceTargetCode::NotificationDeliveriesRepack => MaintenanceTarget::Repack(HeapTarget::NotificationDeliveries),
        MaintenanceTargetCode::PendingSideEffectsRepack => MaintenanceTarget::Repack(HeapTarget::PendingSideEffects),
        MaintenanceTargetCode::TemporalAppendIntentsRepack => MaintenanceTarget::Repack(HeapTarget::TemporalAppendIntents),
        MaintenanceTargetCode::SnapshotNotificationOutboxRepack => MaintenanceTarget::Repack(HeapTarget::SnapshotNotificationOutbox),
        MaintenanceTargetCode::EntitySearchRepack => MaintenanceTarget::Repack(HeapTarget::EntitySearch),
        MaintenanceTargetCode::TemporalAttributeStreamsRepack => MaintenanceTarget::Repack(HeapTarget::TemporalAttributeStreams),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget},
        evidence::target_code::{stored_target, target_of},
    };
    use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
    use andromeda_postgresql_context_store::migration_set::platform::platform_migrations;
    use pretty_assertions::assert_eq;

    #[test]
    fn target_codes_round_trip_for_every_target() {
        let targets = IndexTarget::ALL
            .into_iter()
            .map(MaintenanceTarget::Index)
            .chain(HeapTarget::ALL.into_iter().map(MaintenanceTarget::Heap))
            .chain(HeapTarget::ALL.into_iter().map(MaintenanceTarget::Statistics))
            .chain(HistoryTable::ALL.into_iter().map(MaintenanceTarget::Columnstore))
            .chain(HistoryTable::ALL.into_iter().map(MaintenanceTarget::Rowstore))
            .chain(HeapTarget::ALL.into_iter().map(MaintenanceTarget::Repack));
        for target in targets {
            assert_eq!(target_of(stored_target(target)), target);
        }
        for code in MaintenanceTargetCode::ALL {
            assert_eq!(stored_target(target_of(*code)), *code);
        }
    }

    /// The registry's label is the relation name the controller inspects, with
    /// a suffix for a heap's statistics and a hypertable's columnstore and
    /// rowstore chunks, since a label names one target.
    #[test]
    fn the_stored_label_of_a_target_is_its_relation_name() {
        for code in MaintenanceTargetCode::ALL {
            let target = target_of(*code);
            let expected = match target {
                MaintenanceTarget::Index(_) | MaintenanceTarget::Heap(_) => target.relation_name().to_owned(),
                MaintenanceTarget::Statistics(heap) => format!("{}_statistics", heap.relation_name()),
                MaintenanceTarget::Columnstore(table) => format!("{}_columnstore", table.relation_name()),
                MaintenanceTarget::Rowstore(table) => format!("{}_rowstore", table.relation_name()),
                MaintenanceTarget::Repack(heap) => format!("{}_repack", heap.relation_name()),
            };
            assert_eq!(code.label(), expected);
        }
    }

    #[test]
    fn every_repack_target_round_trips_through_its_own_code() {
        let codes = HeapTarget::ALL.map(|heap| stored_target(MaintenanceTarget::Repack(heap)).code());
        assert_eq!(codes, [46, 47, 48, 49, 50, 51, 52, 53, 54, 55]);
        for heap in HeapTarget::ALL {
            assert_eq!(target_of(stored_target(MaintenanceTarget::Repack(heap))), MaintenanceTarget::Repack(heap));
        }
    }

    /// The platform migration bounds the stored target to the registry's
    /// highest code, so a new target fails here until the check is widened.
    #[test]
    fn the_migration_target_check_ends_at_the_highest_code() {
        let highest = MaintenanceTargetCode::ALL.iter().map(|code| code.code()).max();
        let migrations = platform_migrations();
        let migration = migrations
            .iter()
            .map(|migration| migration.sql.as_str())
            .find(|sql| sql.contains("create table postgresql_maintenance_targets"))
            .unwrap_or_default();
        assert_eq!(highest, Some(55));
        assert!(
            migration.contains("check (target between 1 and 55)"),
            "the targets check names the highest code"
        );
        assert!(
            migration.contains("check (target between 46 and 55)"),
            "the repack runs check names the repack codes"
        );
    }
}
