//! The value columns a `q` Query Term compares against, named by the field
//! they hold rather than by the relation that holds them.
//!
//! A `current_attributes` row, an element of its `sub_attributes` and an
//! `attribute_history` row store one attribute instance's value the same way,
//! in the field its value kind is placed in. A value condition is therefore
//! written once, against a [`ValueColumn`], and the
//! [`InstanceRow`](crate::q_predicate::instance_row::InstanceRow) it reads
//! resolves the field in the relation or element it reads. Only a row splits a
//! field across two columns: a String or a URI past its B-tree bound sits in
//! the field's overflow column.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    string_value_column::StringValueColumn,
    temporal::AttributeHistory,
    uri_value_column::UriValueColumn,
};
use andromeda_postgresql_stored_values::value_placement::ValueField;

/// One value column of a stored attribute instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueColumn {
    /// A field at the one column every relation holds it in, which for a
    /// String or a URI in a row is the column within the B-tree bound.
    Field(ValueField),
    /// A row's String past [`StringValueColumn::Indexed`]'s bound.
    LongText,
    /// A row's Relationship object past [`UriValueColumn::Indexed`]'s bound.
    LongUri,
}

impl ValueColumn {
    /// The column of `field` a value spelled `text` is held in, placed by its
    /// length exactly as the write path placed it. Only the String and URI
    /// fields have an overflow column.
    #[must_use]
    pub const fn holding(field: ValueField, text: &str) -> ValueColumn {
        match field {
            ValueField::String => match StringValueColumn::of(text) {
                StringValueColumn::Indexed => ValueColumn::Field(field),
                StringValueColumn::Overflow => ValueColumn::LongText,
            },
            ValueField::Uri => match UriValueColumn::of(text) {
                UriValueColumn::Indexed => ValueColumn::Field(field),
                UriValueColumn::Overflow => ValueColumn::LongUri,
            },
            ValueField::Number | ValueField::Boolean | ValueField::Geometry | ValueField::Object | ValueField::DateTime => ValueColumn::Field(field),
        }
    }

    /// The overflow column a row holds `field` past its B-tree bound in:
    /// only the String and URI fields have one.
    #[must_use]
    pub const fn overflow_of(field: ValueField) -> Option<ValueColumn> {
        match field {
            ValueField::String => Some(ValueColumn::LongText),
            ValueField::Uri => Some(ValueColumn::LongUri),
            ValueField::Number | ValueField::Boolean | ValueField::Geometry | ValueField::Object | ValueField::DateTime => None,
        }
    }

    /// The field the column holds.
    #[must_use]
    pub const fn field(self) -> ValueField {
        match self {
            ValueColumn::Field(field) => field,
            ValueColumn::LongText => ValueField::String,
            ValueColumn::LongUri => ValueField::Uri,
        }
    }

    /// The `current_attributes` column.
    #[must_use]
    pub const fn in_current(self) -> CurrentAttributes {
        match self {
            ValueColumn::Field(field) => field.in_current(),
            ValueColumn::LongText => StringValueColumn::Overflow.column(),
            ValueColumn::LongUri => UriValueColumn::Overflow.column(),
        }
    }

    /// The `sub_attribute` field. An element has no overflow field, so a long
    /// value is read from its field's one.
    #[must_use]
    pub const fn in_element(self) -> SubAttribute {
        self.field().in_element()
    }

    /// The `attribute_history` column. History has no overflow column, so a
    /// long value is read from its field's one.
    #[must_use]
    pub const fn in_history(self) -> AttributeHistory {
        self.field().in_history()
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::value_column::ValueColumn;
    use andromeda_postgresql_schema::{string_value_column::INDEXED_STRING_BYTE_LIMIT, uri_value_column::INDEXED_URI_BYTE_LIMIT};
    use andromeda_postgresql_stored_values::value_placement::ValueField;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;
    use strum::IntoEnumIterator;

    #[test]
    fn a_string_is_placed_by_its_length_as_the_write_path_places_it() {
        assert_eq!(ValueColumn::holding(ValueField::String, "red"), ValueColumn::Field(ValueField::String));
        assert_eq!(
            ValueColumn::holding(ValueField::String, &"a".repeat(INDEXED_STRING_BYTE_LIMIT + 1)),
            ValueColumn::LongText
        );
    }

    #[test]
    fn a_uri_is_placed_by_its_length_as_the_write_path_places_it() {
        let prefix = "urn:ngsi-ld:Building:";
        let at_limit = format!("{prefix}{}", "a".repeat(INDEXED_URI_BYTE_LIMIT - prefix.len()));
        assert_eq!(ValueColumn::holding(ValueField::Uri, &at_limit), ValueColumn::Field(ValueField::Uri));
        assert_eq!(ValueColumn::holding(ValueField::Uri, &format!("{at_limit}a")), ValueColumn::LongUri);
    }

    /// Only the String and URI fields have an overflow column, so the length
    /// of a value of any other field places nothing.
    #[test]
    fn a_field_without_an_overflow_column_holds_a_value_of_any_length() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        for field in [
            ValueField::Number,
            ValueField::Boolean,
            ValueField::Geometry,
            ValueField::Object,
            ValueField::DateTime,
        ] {
            assert_eq!(ValueColumn::holding(field, &long), ValueColumn::Field(field), "{field:?}");
            assert_eq!(ValueColumn::overflow_of(field), None, "{field:?}");
        }
        assert_eq!(ValueColumn::overflow_of(ValueField::String), Some(ValueColumn::LongText));
        assert_eq!(ValueColumn::overflow_of(ValueField::Uri), Some(ValueColumn::LongUri));
    }

    #[test]
    fn every_field_names_a_row_column_an_element_field_and_a_history_column_of_the_same_name() {
        for field in ValueField::iter() {
            let column = ValueColumn::Field(field);
            assert_eq!(Iden::to_string(&column.in_current()), Iden::to_string(&column.in_history()), "{field:?}");
            assert_eq!(Iden::to_string(&column.in_current()), Iden::to_string(&column.in_element()), "{field:?}");
        }
    }

    /// History and an element have no overflow column, so a long String is
    /// read from `value_string` there.
    #[test]
    fn a_long_string_is_read_from_the_one_history_or_element_string_column() {
        assert_eq!(Iden::to_string(&ValueColumn::LongText.in_current()), "value_long_text");
        assert_eq!(Iden::to_string(&ValueColumn::LongText.in_history()), "value_string");
        assert_eq!(Iden::to_string(&ValueColumn::LongText.in_element()), "value_string");
    }

    /// History has no overflow column, so a long Relationship object is read
    /// from `value_uri` there.
    #[test]
    fn a_long_uri_is_read_from_the_one_history_uri_column() {
        assert_eq!(Iden::to_string(&ValueColumn::LongUri.in_current()), "value_long_uri");
        assert_eq!(Iden::to_string(&ValueColumn::LongUri.in_history()), "value_uri");
    }
}
