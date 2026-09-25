//! The number a maintenance target is stored as.

use crate::candidate::{HeapTarget, IndexTarget, MaintenanceTarget};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;

/// The registry member the platform database stores for a target.
pub(crate) const fn stored_target(target: MaintenanceTarget) -> MaintenanceTargetCode {
    match target {
        MaintenanceTarget::Index(IndexTarget::UqCurrentAttributeValuesGrain) => MaintenanceTargetCode::UqCurrentAttributeValuesGrain,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrNumber) => MaintenanceTargetCode::IdxCavAttrNumber,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrString) => MaintenanceTargetCode::IdxCavAttrString,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrStringTrgm) => MaintenanceTargetCode::IdxCavAttrStringTrgm,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrBool) => MaintenanceTargetCode::IdxCavAttrBool,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrDatetime) => MaintenanceTargetCode::IdxCavAttrDatetime,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrUri) => MaintenanceTargetCode::IdxCavAttrUri,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrGeo) => MaintenanceTargetCode::IdxCavAttrGeo,
        MaintenanceTarget::Index(IndexTarget::IdxCavAttrEntityGeo) => MaintenanceTargetCode::IdxCavAttrEntityGeo,
        MaintenanceTarget::Heap(HeapTarget::CurrentAttributeValues) => MaintenanceTargetCode::CurrentAttributeValues,
        MaintenanceTarget::Heap(HeapTarget::EntityDocuments) => MaintenanceTargetCode::EntityDocuments,
        MaintenanceTarget::Heap(HeapTarget::SubscriptionDeliveryState) => MaintenanceTargetCode::SubscriptionDeliveryState,
    }
}

/// The target a stored registry member names.
pub(crate) const fn target_of(code: MaintenanceTargetCode) -> MaintenanceTarget {
    match code {
        MaintenanceTargetCode::UqCurrentAttributeValuesGrain => MaintenanceTarget::Index(IndexTarget::UqCurrentAttributeValuesGrain),
        MaintenanceTargetCode::IdxCavAttrNumber => MaintenanceTarget::Index(IndexTarget::IdxCavAttrNumber),
        MaintenanceTargetCode::IdxCavAttrString => MaintenanceTarget::Index(IndexTarget::IdxCavAttrString),
        MaintenanceTargetCode::IdxCavAttrStringTrgm => MaintenanceTarget::Index(IndexTarget::IdxCavAttrStringTrgm),
        MaintenanceTargetCode::IdxCavAttrBool => MaintenanceTarget::Index(IndexTarget::IdxCavAttrBool),
        MaintenanceTargetCode::IdxCavAttrDatetime => MaintenanceTarget::Index(IndexTarget::IdxCavAttrDatetime),
        MaintenanceTargetCode::IdxCavAttrUri => MaintenanceTarget::Index(IndexTarget::IdxCavAttrUri),
        MaintenanceTargetCode::IdxCavAttrGeo => MaintenanceTarget::Index(IndexTarget::IdxCavAttrGeo),
        MaintenanceTargetCode::IdxCavAttrEntityGeo => MaintenanceTarget::Index(IndexTarget::IdxCavAttrEntityGeo),
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
