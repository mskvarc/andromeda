//! The value columns a `q` Query Term compares against, named by what they
//! hold rather than by the relation that holds them.
//!
//! A `current_attributes` row, an element of its `sub_attributes` and an
//! `attribute_history` row store one attribute instance's value the same way:
//! one typed field per scalar kind, and `value_object` for every value that is
//! not a single scalar. A value condition is therefore written once, against a
//! [`ValueColumn`], and the
//! [`InstanceRow`](crate::q_predicate::instance_row::InstanceRow) it reads
//! resolves the field in the relation or element it reads.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    projected_string_column::ProjectedStringColumn,
    projected_uri_column::ProjectedUriColumn,
    temporal::AttributeHistory,
};

/// One typed value column of a stored attribute instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueColumn {
    /// A number, held as an exact decimal.
    Number,
    /// A boolean.
    Boolean,
    /// A String short enough for a B-tree entry, and a `Date` or `Time` in its
    /// canonical text.
    String,
    /// A String past [`ProjectedStringColumn::Indexed`]'s bound.
    LongText,
    /// A `DateTime`.
    DateTime,
    /// A Relationship's single object, short enough for a B-tree entry.
    Uri,
    /// A Relationship's single object past [`ProjectedUriColumn::Indexed`]'s
    /// bound.
    LongUri,
    /// Every value that is not a single scalar.
    Object,
}

impl ValueColumn {
    /// The column one String is held in, placed by its length exactly as the
    /// write path placed it.
    #[must_use]
    pub const fn of_string(text: &str) -> ValueColumn {
        match ProjectedStringColumn::of(text) {
            ProjectedStringColumn::Indexed => ValueColumn::String,
            ProjectedStringColumn::Overflow => ValueColumn::LongText,
        }
    }

    /// The column one Relationship object is held in, placed by its length
    /// exactly as the write path placed it.
    #[must_use]
    pub const fn of_uri(uri: &str) -> ValueColumn {
        match ProjectedUriColumn::of(uri) {
            ProjectedUriColumn::Indexed => ValueColumn::Uri,
            ProjectedUriColumn::Overflow => ValueColumn::LongUri,
        }
    }

    /// The `current_attributes` column holding this kind.
    #[must_use]
    pub const fn in_current(self) -> CurrentAttributes {
        match self {
            ValueColumn::Number => CurrentAttributes::ValueNumber,
            ValueColumn::Boolean => CurrentAttributes::ValueBoolean,
            ValueColumn::String => CurrentAttributes::ValueString,
            ValueColumn::LongText => CurrentAttributes::ValueLongText,
            ValueColumn::DateTime => CurrentAttributes::ValueDateTime,
            ValueColumn::Uri => CurrentAttributes::ValueUri,
            ValueColumn::LongUri => CurrentAttributes::ValueLongUri,
            ValueColumn::Object => CurrentAttributes::ValueObject,
        }
    }

    /// The `sub_attribute` field holding this kind.
    ///
    /// An element keeps a String of any length in `value_string` and a URI of
    /// any length in `value_uri`: no B-tree keys on either.
    #[must_use]
    pub const fn in_element(self) -> SubAttribute {
        match self {
            ValueColumn::Number => SubAttribute::ValueNumber,
            ValueColumn::Boolean => SubAttribute::ValueBoolean,
            ValueColumn::String | ValueColumn::LongText => SubAttribute::ValueString,
            ValueColumn::DateTime => SubAttribute::ValueDateTime,
            ValueColumn::Uri | ValueColumn::LongUri => SubAttribute::ValueUri,
            ValueColumn::Object => SubAttribute::ValueObject,
        }
    }

    /// The `attribute_history` column holding this kind.
    ///
    /// History keeps a String of any length in `value_string` and a URI of any
    /// length in `value_uri`: no B-tree keys on either, so neither has an
    /// overflow column, and a long value is read from the one column there is.
    #[must_use]
    pub const fn in_history(self) -> AttributeHistory {
        match self {
            ValueColumn::Number => AttributeHistory::ValueNumber,
            ValueColumn::Boolean => AttributeHistory::ValueBoolean,
            ValueColumn::String | ValueColumn::LongText => AttributeHistory::ValueString,
            ValueColumn::DateTime => AttributeHistory::ValueDateTime,
            ValueColumn::Uri | ValueColumn::LongUri => AttributeHistory::ValueUri,
            ValueColumn::Object => AttributeHistory::ValueObject,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::value_column::ValueColumn;
    use andromeda_postgresql_schema::{projected_string_column::INDEXED_STRING_BYTE_LIMIT, projected_uri_column::INDEXED_URI_BYTE_LIMIT};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn a_string_is_placed_by_its_length_as_the_write_path_places_it() {
        assert_eq!(ValueColumn::of_string("red"), ValueColumn::String);
        assert_eq!(ValueColumn::of_string(&"a".repeat(INDEXED_STRING_BYTE_LIMIT + 1)), ValueColumn::LongText);
    }

    #[test]
    fn a_uri_is_placed_by_its_length_as_the_write_path_places_it() {
        let prefix = "urn:ngsi-ld:Building:";
        let at_limit = format!("{prefix}{}", "a".repeat(INDEXED_URI_BYTE_LIMIT - prefix.len()));
        assert_eq!(ValueColumn::of_uri(&at_limit), ValueColumn::Uri);
        assert_eq!(ValueColumn::of_uri(&format!("{at_limit}a")), ValueColumn::LongUri);
    }

    #[test]
    fn every_kind_names_a_row_column_an_element_field_and_a_history_column_of_the_same_name() {
        for column in [
            ValueColumn::Number,
            ValueColumn::Boolean,
            ValueColumn::String,
            ValueColumn::DateTime,
            ValueColumn::Uri,
            ValueColumn::Object,
        ] {
            assert_eq!(Iden::to_string(&column.in_current()), Iden::to_string(&column.in_history()), "{column:?}");
            assert_eq!(Iden::to_string(&column.in_current()), Iden::to_string(&column.in_element()), "{column:?}");
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
