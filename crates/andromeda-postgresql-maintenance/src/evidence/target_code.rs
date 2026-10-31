//! The number a maintenance target is stored as.

use crate::candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;

/// The registry member the platform database stores for a target.
pub(crate) const fn stored_target(target: MaintenanceTarget) -> MaintenanceTargetCode {
    match target {
        MaintenanceTarget::Index(IndexTarget::UqCurrentAttributesGrain) => MaintenanceTargetCode::UqCurrentAttributesGrain,
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
    }
}

/// The target a stored registry member names.
pub(crate) const fn target_of(code: MaintenanceTargetCode) -> MaintenanceTarget {
    match code {
        MaintenanceTargetCode::UqCurrentAttributesGrain => MaintenanceTarget::Index(IndexTarget::UqCurrentAttributesGrain),
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
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget},
        evidence::target_code::{stored_target, target_of},
    };
    use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn target_codes_round_trip_for_every_target() {
        let targets = IndexTarget::ALL
            .into_iter()
            .map(MaintenanceTarget::Index)
            .chain(HeapTarget::ALL.into_iter().map(MaintenanceTarget::Heap))
            .chain(HeapTarget::ALL.into_iter().map(MaintenanceTarget::Statistics))
            .chain(HistoryTable::ALL.into_iter().map(MaintenanceTarget::Columnstore))
            .chain(HistoryTable::ALL.into_iter().map(MaintenanceTarget::Rowstore));
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
            };
            assert_eq!(code.label(), expected);
        }
    }
}
