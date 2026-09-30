//! The persisted value shape of one attribute-history row.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// Which typed column of an attribute-history row holds its value.
    ///
    /// One member per typed value column, plus the reserved
    /// [`Tombstone`](ValueKind::Tombstone). The number is not a convenience
    /// index: the rebuilt history table constrains the row so that the member
    /// and the populated column agree, which makes a stored value whose
    /// declared shape disagrees with where it actually sits unrepresentable
    /// rather than something a reader has to guess its way around.
    ///
    /// [`Tombstone`](ValueKind::Tombstone) is permanently reserved at zero. A
    /// tombstone row carries a deletion timestamp and no typed value at all;
    /// the null representation `urn:ngsi-ld:null` is synthesised from the
    /// attribute family at render time (clauses 4.5.7 and 4.5.8) and is never
    /// stored as though it were a URI a client had supplied.
    ///
    /// Andromeda owns this vocabulary rather than the specification, so the
    /// labels are kebab-case.
    ValueKind {
        frozen_prefix: 8,
        frozen_digest: 0xc60e_1e69_c540_c739,
        codes: {
            /// The row records a deletion and carries no typed value.
            Tombstone = 0, "tombstone";
            /// The row's value is an arbitrary-precision number.
            Number = 1, "number";
            /// The row's value is a string.
            String = 2, "string";
            /// The row's value is a boolean.
            Boolean = 3, "boolean";
            /// The row's value is a geometry.
            Geometry = 4, "geometry";
            /// The row's value is a JSON object or array.
            Object = 5, "object";
            /// The row's value is a point in time.
            DateTime = 6, "date-time";
            /// The row's value is a URI, which is how a relationship object is held.
            Uri = 7, "uri";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value_kind::ValueKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_tombstone_is_permanently_reserved_at_zero() {
        assert_eq!(ValueKind::Tombstone.code(), 0);
        assert_eq!(ValueKind::try_from(0), Ok(ValueKind::Tombstone));
    }

    #[test]
    fn no_value_shape_other_than_the_tombstone_is_numbered_zero() {
        let zeroes = ValueKind::ALL.iter().filter(|kind| kind.code() == 0).count();
        assert_eq!(zeroes, 1);
        assert!(ValueKind::ALL.iter().all(|kind| kind.code() >= 0));
    }

    #[test]
    fn every_typed_value_column_has_exactly_one_member() {
        assert_eq!(ValueKind::ALL.len(), 8);
    }

    #[test]
    fn a_negative_number_is_not_a_value_shape() {
        assert!(ValueKind::try_from(-1).is_err());
    }

    #[test]
    fn a_value_shape_a_newer_release_might_add_fails_to_decode() {
        assert!(ValueKind::try_from(8).is_err());
    }
}
