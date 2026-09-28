//! The value columns a `q` Query Term compares against, named by what they
//! hold rather than by the relation that holds them.
//!
//! The current-state projection and the temporal history store one attribute
//! instance's value the same way: one typed column per scalar kind, and
//! `value_object` for every value that is not a single scalar. A value
//! condition is therefore written once, against a [`ValueColumn`], and the
//! [`InstanceScan`](crate::q_predicate::instance_scan::InstanceScan) it runs
//! under resolves the column in the relation it scans.

use andromeda_postgresql_schema::{current::CurrentAttributeValues, projected_string_column::ProjectedStringColumn, temporal::AttributeHistory};

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
    /// A Relationship's single object.
    Uri,
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

    /// The `current_attribute_values` column holding this kind.
    #[must_use]
    pub const fn in_projection(self) -> CurrentAttributeValues {
        match self {
            ValueColumn::Number => CurrentAttributeValues::ValueNumber,
            ValueColumn::Boolean => CurrentAttributeValues::ValueBoolean,
            ValueColumn::String => CurrentAttributeValues::ValueString,
            ValueColumn::LongText => CurrentAttributeValues::ValueLongText,
            ValueColumn::DateTime => CurrentAttributeValues::ValueDateTime,
            ValueColumn::Uri => CurrentAttributeValues::ValueUri,
            ValueColumn::Object => CurrentAttributeValues::ValueObject,
        }
    }

    /// The `attribute_history` column holding this kind.
    ///
    /// History keeps a String of any length in `value_string`: no B-tree keys
    /// on it, so it has no overflow column, and a long String is read from the
    /// one column there is.
    #[must_use]
    pub const fn in_history(self) -> AttributeHistory {
        match self {
            ValueColumn::Number => AttributeHistory::ValueNumber,
            ValueColumn::Boolean => AttributeHistory::ValueBoolean,
            ValueColumn::String | ValueColumn::LongText => AttributeHistory::ValueString,
            ValueColumn::DateTime => AttributeHistory::ValueDateTime,
            ValueColumn::Uri => AttributeHistory::ValueUri,
            ValueColumn::Object => AttributeHistory::ValueObject,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::value_column::ValueColumn;
    use andromeda_postgresql_schema::projected_string_column::INDEXED_STRING_BYTE_LIMIT;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn a_string_is_placed_by_its_length_as_the_write_path_places_it() {
        assert_eq!(ValueColumn::of_string("red"), ValueColumn::String);
        assert_eq!(ValueColumn::of_string(&"a".repeat(INDEXED_STRING_BYTE_LIMIT + 1)), ValueColumn::LongText);
    }

    #[test]
    fn every_kind_names_a_projection_column_and_a_history_column_of_the_same_name() {
        for column in [
            ValueColumn::Number,
            ValueColumn::Boolean,
            ValueColumn::String,
            ValueColumn::DateTime,
            ValueColumn::Uri,
            ValueColumn::Object,
        ] {
            assert_eq!(Iden::to_string(&column.in_projection()), Iden::to_string(&column.in_history()), "{column:?}");
        }
    }

    /// History has no overflow column, so a long String is read from
    /// `value_string` there.
    #[test]
    fn a_long_string_is_read_from_the_one_history_string_column() {
        assert_eq!(Iden::to_string(&ValueColumn::LongText.in_projection()), "value_long_text");
        assert_eq!(Iden::to_string(&ValueColumn::LongText.in_history()), "value_string");
    }
}
