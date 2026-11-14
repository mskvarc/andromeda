//! Where a stored attribute instance holds its value, by value kind.
//!
//! A `current_attributes` row, an element of its `sub_attributes` and an
//! `attribute_history` row hold a value alike: one typed field per scalar
//! shape, the geometry fields, and `value_object` for every value that is not
//! one scalar. Several kinds share a field, so every statement that writes a
//! value and every one that compares it must agree on the field each kind is
//! in, and on when a reader names the kind beside the field. [`value_placement`]
//! states both, once, and both sides derive from it.
//!
//! A row's length split belongs to the row, not to the kind: a String or a URI
//! past its B-tree bound sits in an overflow column of its field, decided from
//! the length alone, and an element and a history row have no overflow column.

use crate::value_kind::ValueKind;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    string_value_column::StringValueColumn,
    temporal::AttributeHistory,
    uri_value_column::UriValueColumn,
};
use strum::EnumIter;

/// One value field every stored instance has, whichever relation holds it.
///
/// Declared in the order a reader folds the fields in when it reads a value as
/// text.
#[derive(Clone, Copy, Debug, EnumIter, Eq, Hash, PartialEq)]
pub enum ValueField {
    /// `value_number`: an exact decimal, `INF`, `-INF` and `NaN` as the
    /// `numeric` values `Infinity`, `-Infinity` and `NaN`.
    Number,
    /// `value_string`: a String as it is, a `Date` in its one spelling and a
    /// `Time` in its fixed-width storage encoding.
    String,
    /// `value_boolean`.
    Boolean,
    /// `value_geometry`, with the reach and the members recorded beside it.
    Geometry,
    /// `value_object`: every value that is not one scalar, in the stored JSON
    /// of the structured value codec.
    Object,
    /// `value_date_time`: an instant at microsecond resolution.
    DateTime,
    /// `value_uri`: the single object of a Relationship.
    Uri,
}

impl ValueField {
    /// The `current_attributes` column holding the field; for a String or a
    /// URI, the column within the B-tree bound.
    #[must_use]
    pub const fn in_current(self) -> CurrentAttributes {
        match self {
            ValueField::Number => CurrentAttributes::ValueNumber,
            ValueField::String => StringValueColumn::Indexed.column(),
            ValueField::Boolean => CurrentAttributes::ValueBoolean,
            ValueField::Geometry => CurrentAttributes::ValueGeometry,
            ValueField::Object => CurrentAttributes::ValueObject,
            ValueField::DateTime => CurrentAttributes::ValueDateTime,
            ValueField::Uri => UriValueColumn::Indexed.column(),
        }
    }

    /// The `sub_attribute` field holding the field. An element keeps a String
    /// and a URI of any length in its one field, since no B-tree keys either.
    #[must_use]
    pub const fn in_element(self) -> SubAttribute {
        match self {
            ValueField::Number => SubAttribute::ValueNumber,
            ValueField::String => SubAttribute::ValueString,
            ValueField::Boolean => SubAttribute::ValueBoolean,
            ValueField::Geometry => SubAttribute::ValueGeometry,
            ValueField::Object => SubAttribute::ValueObject,
            ValueField::DateTime => SubAttribute::ValueDateTime,
            ValueField::Uri => SubAttribute::ValueUri,
        }
    }

    /// The `attribute_history` column holding the field. History keeps a
    /// String and a URI of any length in its one column, since no B-tree keys
    /// either.
    #[must_use]
    pub const fn in_history(self) -> AttributeHistory {
        match self {
            ValueField::Number => AttributeHistory::ValueNumber,
            ValueField::String => AttributeHistory::ValueString,
            ValueField::Boolean => AttributeHistory::ValueBoolean,
            ValueField::Geometry => AttributeHistory::ValueGeometry,
            ValueField::Object => AttributeHistory::ValueObject,
            ValueField::DateTime => AttributeHistory::ValueDateTime,
            ValueField::Uri => AttributeHistory::ValueUri,
        }
    }
}

/// How a reader reads the values of one kind from the field it shares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToldApart {
    /// Every kind the field holds is of one data type to a clause 4.9
    /// comparison, so reading the field reads the kind.
    Unneeded,
    /// The field holds kinds of different data types, so a reader names the
    /// kind's `value_kind` beside the field.
    ByValueKind,
}

/// Where a stored instance of one value kind holds its value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValuePlacement {
    /// One value field.
    Field {
        /// The field holding the value.
        field: ValueField,
        /// Whether a reader names the kind beside the field.
        told_apart: ToldApart,
    },
    /// The proof fields of a `sub_attribute` element. A data-integrity seal is
    /// only ever a sub-attribute (clause 4.5.2.2), so no row holds one.
    Proof,
    /// No field: a tombstone records the instant of a deletion and no value.
    Valueless,
}

/// Where an instance of `kind` holds its value.
///
/// A `Date` and a `Time` share the String field with a String, and clause 4.9
/// lets a target match only a Query Term of its own data type, so a reader of
/// any of the three names its kind. A declared number shares the number field
/// with a JSON number, and a declared Boolean the boolean field with a JSON
/// boolean, each compared as the number or the Boolean it names, so neither
/// field is read by kind.
#[must_use]
pub const fn value_placement(kind: ValueKind) -> ValuePlacement {
    match kind {
        ValueKind::Tombstone => ValuePlacement::Valueless,
        ValueKind::Number | ValueKind::DeclaredNumber => ValuePlacement::Field {
            field: ValueField::Number,
            told_apart: ToldApart::Unneeded,
        },
        ValueKind::String | ValueKind::Date | ValueKind::Time => ValuePlacement::Field {
            field: ValueField::String,
            told_apart: ToldApart::ByValueKind,
        },
        ValueKind::Boolean | ValueKind::DeclaredBoolean => ValuePlacement::Field {
            field: ValueField::Boolean,
            told_apart: ToldApart::Unneeded,
        },
        ValueKind::Geometry => ValuePlacement::Field {
            field: ValueField::Geometry,
            told_apart: ToldApart::Unneeded,
        },
        ValueKind::Object => ValuePlacement::Field {
            field: ValueField::Object,
            told_apart: ToldApart::Unneeded,
        },
        ValueKind::DateTime => ValuePlacement::Field {
            field: ValueField::DateTime,
            told_apart: ToldApart::Unneeded,
        },
        ValueKind::Uri => ValuePlacement::Field {
            field: ValueField::Uri,
            told_apart: ToldApart::Unneeded,
        },
        ValueKind::DataIntegrityProof => ValuePlacement::Proof,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        value_kind::ValueKind,
        value_placement::{ToldApart, ValueField, ValuePlacement, value_placement},
    };
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    /// The field `kind` is held in, for a kind held in one.
    fn field_of(kind: ValueKind) -> Option<ValueField> {
        match value_placement(kind) {
            ValuePlacement::Field { field, .. } => Some(field),
            ValuePlacement::Proof | ValuePlacement::Valueless => None,
        }
    }

    /// Clause 4.9 matches a target only with a Query Term of its own data
    /// type, and a String, a `Date` and a `Time` are three.
    #[test]
    fn a_string_a_date_and_a_time_share_the_string_field_and_are_each_read_by_kind() {
        for kind in [ValueKind::String, ValueKind::Date, ValueKind::Time] {
            assert_eq!(
                value_placement(kind),
                ValuePlacement::Field {
                    field: ValueField::String,
                    told_apart: ToldApart::ByValueKind
                },
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_declared_number_and_a_declared_boolean_share_the_field_of_what_they_name() {
        for (kinds, field) in [
            ([ValueKind::Number, ValueKind::DeclaredNumber], ValueField::Number),
            ([ValueKind::Boolean, ValueKind::DeclaredBoolean], ValueField::Boolean),
        ] {
            for kind in kinds {
                assert_eq!(
                    value_placement(kind),
                    ValuePlacement::Field {
                        field,
                        told_apart: ToldApart::Unneeded
                    },
                    "{kind:?}"
                );
            }
        }
    }

    #[test]
    fn only_the_tombstone_holds_no_value_and_only_a_seal_is_held_in_proof_fields() {
        let valueless: Vec<ValueKind> = ValueKind::ALL
            .iter()
            .copied()
            .filter(|kind| value_placement(*kind) == ValuePlacement::Valueless)
            .collect();
        let proof: Vec<ValueKind> = ValueKind::ALL
            .iter()
            .copied()
            .filter(|kind| value_placement(*kind) == ValuePlacement::Proof)
            .collect();
        assert_eq!(valueless, [ValueKind::Tombstone]);
        assert_eq!(proof, [ValueKind::DataIntegrityProof]);
    }

    /// A field no kind is placed in would be a column nothing writes.
    #[test]
    fn every_field_holds_some_kind() {
        for field in ValueField::iter() {
            assert!(ValueKind::ALL.iter().copied().any(|kind| field_of(kind) == Some(field)), "{field:?}");
        }
    }

    /// Only a kind told apart by `value_kind` shares its field with a kind of
    /// another data type: a field read for a kind without naming it holds that
    /// kind's data type alone.
    #[test]
    fn a_field_holding_a_kind_read_by_kind_reads_every_kind_it_holds_by_kind() {
        for field in ValueField::iter() {
            let guards: Vec<ToldApart> = ValueKind::ALL
                .iter()
                .copied()
                .filter_map(|kind| match value_placement(kind) {
                    ValuePlacement::Field { field: held, told_apart } if held == field => Some(told_apart),
                    ValuePlacement::Field { .. } | ValuePlacement::Proof | ValuePlacement::Valueless => None,
                })
                .collect();
            assert!(guards.windows(2).all(|pair| pair[0] == pair[1]), "{field:?}: {guards:?}");
        }
    }
}
