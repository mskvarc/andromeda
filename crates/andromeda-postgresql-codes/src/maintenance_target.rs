//! The persisted relation one maintenance target row describes.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// A relation the opt-in PostgreSQL maintenance controller may act on.
    ///
    /// The labels are the relation names the controller inspects, which is how an
    /// operator recognises a target in the platform tables.
    MaintenanceTargetCode {
        frozen_prefix: 13,
        frozen_digest: 0xe988_1329_b9bc_1c59,
        codes: {
            /// The current attributes' grain constraint index.
            UqCurrentAttributesGrain = 1, "uq_current_attributes_grain";
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
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_target::MaintenanceTargetCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceTargetCode::ALL.len(), 13);
        assert_eq!(MaintenanceTargetCode::UqCurrentAttributesGrain.code(), 1);
        assert_eq!(MaintenanceTargetCode::IdxCaSubEqKeys.code(), 10);
        assert_eq!(MaintenanceTargetCode::SubscriptionDeliveryState.code(), 13);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceTargetCode::try_from(0).is_err());
        assert!(MaintenanceTargetCode::try_from(14).is_err());
    }
}
