//! The persisted relation one maintenance target row describes.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// A relation the opt-in PostgreSQL maintenance controller may act on.
    ///
    /// The labels are the relation names the controller inspects, which is how an
    /// operator recognises a target in the platform tables.
    MaintenanceTargetCode {
        frozen_prefix: 12,
        frozen_digest: 0x5ef9_4d6f_5234_154d,
        codes: {
            /// The current projection's grain constraint index.
            UqCurrentAttributeValuesGrain = 1, "uq_current_attribute_values_grain";
            /// The current projection's number index.
            IdxCavAttrNumber = 2, "idx_cav_attr_number";
            /// The current projection's string index.
            IdxCavAttrString = 3, "idx_cav_attr_string";
            /// The current projection's trigram index.
            IdxCavAttrStringTrgm = 4, "idx_cav_attr_string_trgm";
            /// The current projection's boolean index.
            IdxCavAttrBool = 5, "idx_cav_attr_bool";
            /// The current projection's date-time index.
            IdxCavAttrDatetime = 6, "idx_cav_attr_datetime";
            /// The current projection's URI index.
            IdxCavAttrUri = 7, "idx_cav_attr_uri";
            /// The current projection's geometry index.
            IdxCavAttrGeo = 8, "idx_cav_attr_geo";
            /// The current projection's entity-geometry index.
            IdxCavAttrEntityGeo = 9, "idx_cav_attr_entity_geo";
            /// The current projection's heap.
            CurrentAttributeValues = 10, "current_attribute_values";
            /// The entity payload heap.
            EntityDocuments = 11, "entity_documents";
            /// The subscription delivery-state heap.
            SubscriptionDeliveryState = 12, "subscription_delivery_state";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maintenance_target::MaintenanceTargetCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(MaintenanceTargetCode::ALL.len(), 12);
        assert_eq!(MaintenanceTargetCode::UqCurrentAttributeValuesGrain.code(), 1);
        assert_eq!(MaintenanceTargetCode::SubscriptionDeliveryState.code(), 12);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(MaintenanceTargetCode::try_from(0).is_err());
        assert!(MaintenanceTargetCode::try_from(13).is_err());
    }
}
