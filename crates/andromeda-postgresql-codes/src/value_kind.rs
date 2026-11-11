//! The persisted value shape of one attribute-history row.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};
use andromeda_postgresql_schema::temporal::AttributeHistory;

compact_code_registry! {
    /// Which typed column of an attribute-history row holds its value.
    ///
    /// One member per typed value column, plus the two time value data types
    /// the string column holds apart from a String, the declared number and
    /// Boolean the number and boolean columns hold apart from a JSON number
    /// and boolean, and the reserved [`Tombstone`](ValueKind::Tombstone). The number is not a convenience
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
            /// The row's value is a value declared as a clause 4.6.3 `Date`, held
            /// as its one spelling in the string column.
            Date = 8, "date";
            /// The row's value is a value declared as a clause 4.6.3 `Time`, held
            /// in the string column in its fixed-width storage encoding
            /// `HH:MM:SS.ffffffZ`, whose byte order is chronological.
            Time = 9, "time";
            /// The row's value is a JSON string declared as an XSD number,
            /// held in the number column, `INF`, `-INF` and `NaN` as the
            /// `numeric` values `Infinity`, `-Infinity` and `NaN`.
            DeclaredNumber = 10, "declared-number";
            /// The row's value is a JSON string declared as `xsd:boolean`, held
            /// in the boolean column.
            DeclaredBoolean = 11, "declared-boolean";
            /// The value of a data-integrity seal (clause 4.5.2.2): a W3C Data
            /// Integrity proof, held in the proof fields of a `sub_attribute`
            /// element with its two seals. A seal is only ever a
            /// sub-attribute, so no row holds this kind.
            DataIntegrityProof = 12, "data-integrity-proof";
        }
    }
}

impl ValueKind {
    /// The `attribute_history` column a row of this kind holds its value in;
    /// `None` for the tombstone, which holds none, and for a seal, which no
    /// row holds. A `Date` and a `Time` share the string column with a String,
    /// a declared number the number column with a JSON number, and a declared
    /// Boolean the boolean column with a JSON boolean.
    #[must_use]
    pub const fn history_column(self) -> Option<AttributeHistory> {
        match self {
            ValueKind::Tombstone | ValueKind::DataIntegrityProof => None,
            ValueKind::Number | ValueKind::DeclaredNumber => Some(AttributeHistory::ValueNumber),
            ValueKind::String | ValueKind::Date | ValueKind::Time => Some(AttributeHistory::ValueString),
            ValueKind::Boolean | ValueKind::DeclaredBoolean => Some(AttributeHistory::ValueBoolean),
            ValueKind::Geometry => Some(AttributeHistory::ValueGeometry),
            ValueKind::Object => Some(AttributeHistory::ValueObject),
            ValueKind::DateTime => Some(AttributeHistory::ValueDateTime),
            ValueKind::Uri => Some(AttributeHistory::ValueUri),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value_kind::ValueKind;
    use andromeda_postgresql_schema::temporal::AttributeHistory;

    /// Only the tombstone and a seal hold no history column; three kinds
    /// share the string column and two each the number and boolean columns.
    #[test]
    fn every_kind_but_the_tombstone_and_a_seal_names_the_column_it_holds_its_value_in() {
        assert_eq!(ValueKind::Tombstone.history_column(), None);
        assert_eq!(ValueKind::DataIntegrityProof.history_column(), None);
        for kind in [ValueKind::String, ValueKind::Date, ValueKind::Time] {
            assert_eq!(kind.history_column(), Some(AttributeHistory::ValueString), "{kind:?}");
        }
        for kind in [ValueKind::Number, ValueKind::DeclaredNumber] {
            assert_eq!(kind.history_column(), Some(AttributeHistory::ValueNumber), "{kind:?}");
        }
        for kind in [ValueKind::Boolean, ValueKind::DeclaredBoolean] {
            assert_eq!(kind.history_column(), Some(AttributeHistory::ValueBoolean), "{kind:?}");
        }
        let columns: Vec<AttributeHistory> = ValueKind::ALL.iter().filter_map(|kind| kind.history_column()).collect();
        let distinct = columns.iter().enumerate().filter(|(index, column)| !columns[..*index].contains(column)).count();
        assert_eq!(distinct, 7);
    }
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
    fn every_value_shape_has_exactly_one_member() {
        assert_eq!(ValueKind::ALL.len(), 13);
    }

    #[test]
    fn a_negative_number_is_not_a_value_shape() {
        assert!(ValueKind::try_from(-1).is_err());
    }

    #[test]
    fn a_value_shape_a_newer_release_might_add_fails_to_decode() {
        assert!(ValueKind::try_from(13).is_err());
    }
}
