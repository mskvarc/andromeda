//! The persisted lifecycle transition of one notification event.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// What one write did to the Entity a notification event records.
    ///
    /// Clause 5.2.12's entity-level triggers read it: a creation raises
    /// `entityCreated`, a deletion `entityDeleted`, and a modification raises
    /// only the attribute-level triggers of the instances it changed.
    EntityLifecycleCode {
        frozen_at: (1, 0, 0),
        frozen_prefix: 3,
        frozen_digest: 0x685c_8194_3bd1_b6a1,
        codes: {
            /// The Entity did not exist before the write.
            Created = 1, "created";
            /// The Entity existed before the write and still exists after it.
            Modified = 2, "modified";
            /// The Entity existed before the write and does not exist after it.
            Deleted = 3, "deleted";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_lifecycle::EntityLifecycleCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_lifecycle_transitions_are_numbered_as_the_migration_writes_them() {
        assert_eq!(EntityLifecycleCode::Created.code(), 1);
        assert_eq!(EntityLifecycleCode::Modified.code(), 2);
        assert_eq!(EntityLifecycleCode::Deleted.code(), 3);
    }

    #[test]
    fn a_transition_a_newer_release_might_add_fails_to_decode() {
        assert!(EntityLifecycleCode::try_from(0).is_err());
        assert!(EntityLifecycleCode::try_from(4).is_err());
    }
}
