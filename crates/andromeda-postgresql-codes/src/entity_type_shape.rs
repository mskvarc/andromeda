//! The persisted shape of one registered `EntityInfo` type.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// How a registration's `EntityInfo` wrote its Table 5.2.8-1 `type`: one
    /// name, or a JSON array of names (which may hold one), read back in the
    /// shape it was written in.
    EntityTypeShapeCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 2,
        frozen_digest: 0x2c4b_609e_ccbc_e9c0,
        codes: {
            /// One type name.
            Single = 1, "single";
            /// An array of type names.
            Several = 2, "several";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_type_shape::EntityTypeShapeCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(EntityTypeShapeCode::ALL.len(), 2);
        assert_eq!(EntityTypeShapeCode::Single.code(), 1);
        assert_eq!(EntityTypeShapeCode::Several.code(), 2);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(EntityTypeShapeCode::try_from(0).is_err());
        assert!(EntityTypeShapeCode::try_from(3).is_err());
    }
}
