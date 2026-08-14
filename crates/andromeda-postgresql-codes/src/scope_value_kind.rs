//! The persisted value shape of one scope-history row.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// Which value a scope-history row carries.
    ///
    /// A scope is not an attribute: clause 4.5.5 gives it no dataset dimension,
    /// no attribute family and no catalogue entry, so scope history has no
    /// attribute-kind column and its value vocabulary is its own rather than a
    /// borrowed subset of the attribute one. Sharing a registry between the two
    /// would tie a scope row's meaning to a number assigned for an attribute
    /// column that scope history does not have.
    ///
    /// [`Tombstone`](ScopeValueKind::Tombstone) is reserved at zero for the
    /// same reason it is in the attribute registry: a scope deletion is a row
    /// carrying a deletion timestamp and no path, not an invented sentinel
    /// path.
    ScopeValueKind {
        frozen_prefix: 2,
        frozen_digest: 0x7acb_158d_0fc0_c6e5,
        codes: {
            /// The row records a scope deletion and carries no path.
            Tombstone = 0, "tombstone";
            /// The row carries a scope path.
            ScopePath = 1, "scope-path";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::scope_value_kind::ScopeValueKind;

    #[test]
    fn the_tombstone_is_permanently_reserved_at_zero() {
        assert_eq!(ScopeValueKind::Tombstone.code(), 0);
        assert_eq!(ScopeValueKind::try_from(0), Ok(ScopeValueKind::Tombstone));
    }

    #[test]
    fn the_scope_registry_has_exactly_two_members() {
        assert_eq!(ScopeValueKind::ALL.len(), 2);
        assert_eq!(ScopeValueKind::ScopePath.code(), 1);
    }

    #[test]
    fn an_attribute_value_shape_is_not_a_scope_value_shape() {
        assert!(ScopeValueKind::try_from(2).is_err());
    }
}
