//! The number a maintenance target is stored as.

use crate::candidate::{HeapTarget, IndexTarget, MaintenanceTarget};
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
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
        evidence::target_code::{stored_target, target_of},
    };
    use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn target_codes_round_trip_for_every_target() {
        let targets = IndexTarget::ALL
            .into_iter()
            .map(MaintenanceTarget::Index)
            .chain(HeapTarget::ALL.into_iter().map(MaintenanceTarget::Heap));
        for target in targets {
            assert_eq!(target_of(stored_target(target)), target);
        }
        for code in MaintenanceTargetCode::ALL {
            assert_eq!(stored_target(target_of(*code)), *code);
        }
    }

    /// The registry's label is the relation name the controller inspects.
    #[test]
    fn the_stored_label_of_a_target_is_its_relation_name() {
        for code in MaintenanceTargetCode::ALL {
            assert_eq!(code.label(), target_of(*code).relation_name());
        }
    }
}
