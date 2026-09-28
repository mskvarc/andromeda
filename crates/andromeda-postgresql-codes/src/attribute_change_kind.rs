//! The persisted transition of one Attribute instance a notification event
//! records.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// What one write did to one Attribute instance (clause 4.5.5 identity).
    ///
    /// Each transition raises the clause 5.2.12 attribute-level trigger of the
    /// same name.
    AttributeChangeKindCode {
        frozen_prefix: 3,
        frozen_digest: 0xde2a_88d2_e98d_4485,
        codes: {
            /// The instance was absent and is now present.
            Created = 1, "created";
            /// The instance remains present and its node changed.
            Updated = 2, "updated";
            /// The instance was present and is now absent.
            Deleted = 3, "deleted";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_change_kind::AttributeChangeKindCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_instance_transitions_are_numbered_as_the_migration_writes_them() {
        assert_eq!(AttributeChangeKindCode::Created.code(), 1);
        assert_eq!(AttributeChangeKindCode::Updated.code(), 2);
        assert_eq!(AttributeChangeKindCode::Deleted.code(), 3);
    }

    #[test]
    fn a_transition_a_newer_release_might_add_fails_to_decode() {
        assert!(AttributeChangeKindCode::try_from(0).is_err());
        assert!(AttributeChangeKindCode::try_from(4).is_err());
    }
}
