//! The number a maintenance target is stored as.

use crate::candidate::{HeapTarget, IndexTarget, MaintenanceTarget};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;

/// The registry member the platform database stores for a target.
pub(crate) const fn stored_target(target: MaintenanceTarget) -> MaintenanceTargetCode {
    match target {
        MaintenanceTarget::Index(IndexTarget::UqCurrentAttributeValuesGrain) => MaintenanceTargetCode::UqCurrentAttributeValuesGrain,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueNumber) => MaintenanceTargetCode::IdxCavValueNumber,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueString) => MaintenanceTargetCode::IdxCavValueString,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueStringTrgm) => MaintenanceTargetCode::IdxCavValueStringTrgm,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueBoolean) => MaintenanceTargetCode::IdxCavValueBoolean,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueDateTime) => MaintenanceTargetCode::IdxCavValueDateTime,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueUri) => MaintenanceTargetCode::IdxCavValueUri,
        MaintenanceTarget::Index(IndexTarget::IdxCavValueGeometry) => MaintenanceTargetCode::IdxCavValueGeometry,
        MaintenanceTarget::Index(IndexTarget::IdxCavEntityGeometry) => MaintenanceTargetCode::IdxCavEntityGeometry,
        MaintenanceTarget::Heap(HeapTarget::CurrentAttributeValues) => MaintenanceTargetCode::CurrentAttributeValues,
        MaintenanceTarget::Heap(HeapTarget::EntityDocuments) => MaintenanceTargetCode::EntityDocuments,
        MaintenanceTarget::Heap(HeapTarget::SubscriptionDeliveryState) => MaintenanceTargetCode::SubscriptionDeliveryState,
    }
}

/// The target a stored registry member names.
pub(crate) const fn target_of(code: MaintenanceTargetCode) -> MaintenanceTarget {
    match code {
        MaintenanceTargetCode::UqCurrentAttributeValuesGrain => MaintenanceTarget::Index(IndexTarget::UqCurrentAttributeValuesGrain),
        MaintenanceTargetCode::IdxCavValueNumber => MaintenanceTarget::Index(IndexTarget::IdxCavValueNumber),
        MaintenanceTargetCode::IdxCavValueString => MaintenanceTarget::Index(IndexTarget::IdxCavValueString),
        MaintenanceTargetCode::IdxCavValueStringTrgm => MaintenanceTarget::Index(IndexTarget::IdxCavValueStringTrgm),
        MaintenanceTargetCode::IdxCavValueBoolean => MaintenanceTarget::Index(IndexTarget::IdxCavValueBoolean),
        MaintenanceTargetCode::IdxCavValueDateTime => MaintenanceTarget::Index(IndexTarget::IdxCavValueDateTime),
        MaintenanceTargetCode::IdxCavValueUri => MaintenanceTarget::Index(IndexTarget::IdxCavValueUri),
        MaintenanceTargetCode::IdxCavValueGeometry => MaintenanceTarget::Index(IndexTarget::IdxCavValueGeometry),
        MaintenanceTargetCode::IdxCavEntityGeometry => MaintenanceTarget::Index(IndexTarget::IdxCavEntityGeometry),
        MaintenanceTargetCode::CurrentAttributeValues => MaintenanceTarget::Heap(HeapTarget::CurrentAttributeValues),
        MaintenanceTargetCode::EntityDocuments => MaintenanceTarget::Heap(HeapTarget::EntityDocuments),
        MaintenanceTargetCode::SubscriptionDeliveryState => MaintenanceTarget::Heap(HeapTarget::SubscriptionDeliveryState),
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
    fn every_target_survives_a_round_trip_through_its_stored_number() {
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
