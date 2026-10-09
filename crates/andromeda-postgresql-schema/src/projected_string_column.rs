//! Which of the two string columns of `current_attributes` a String
//! value is held in.
//!
//! A B-tree refuses an entry past 2704 bytes (SQLSTATE 54000), and
//! `idx_ca_value_string` keys on `value_string`, so a String longer than
//! [`INDEXED_STRING_BYTE_LIMIT`] is held in `value_long_text` instead. The
//! migration's two length checks make that placement canonical: every string
//! has exactly one column it may sit in. The writer places a value through
//! [`ProjectedStringColumn::of`] and the `q` compiler routes an equality
//! through the same function, which is what makes reading only the operand's
//! own column exact.

use crate::current::CurrentAttributes;

/// The greatest length, in UTF-8 octets, a String held in `value_string` may
/// have.
///
/// 2048 leaves the `(attribute_name_key, value_string)` entry of
/// `idx_ca_value_string` well inside the 2704 bytes a B-tree entry may take. A
/// unit test in `migration_set` pins the literal the `current_attributes` migration states to this
/// constant.
pub const INDEXED_STRING_BYTE_LIMIT: usize = 2048;

/// The column one String value is held in.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProjectedStringColumn {
    /// `value_string`, keyed by a B-tree: at most [`INDEXED_STRING_BYTE_LIMIT`]
    /// octets.
    Indexed,
    /// `value_long_text`, past what a B-tree entry may hold.
    Overflow,
}

impl ProjectedStringColumn {
    /// The column `text` is held in.
    #[must_use]
    pub const fn of(text: &str) -> ProjectedStringColumn {
        if text.len() <= INDEXED_STRING_BYTE_LIMIT {
            ProjectedStringColumn::Indexed
        } else {
            ProjectedStringColumn::Overflow
        }
    }

    /// The column this placement names.
    #[must_use]
    pub const fn column(self) -> CurrentAttributes {
        match self {
            ProjectedStringColumn::Indexed => CurrentAttributes::ValueString,
            ProjectedStringColumn::Overflow => CurrentAttributes::ValueLongText,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::projected_string_column::{INDEXED_STRING_BYTE_LIMIT, ProjectedStringColumn};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn a_string_of_exactly_the_limit_is_indexed() {
        assert_eq!(
            ProjectedStringColumn::of(&"a".repeat(INDEXED_STRING_BYTE_LIMIT)),
            ProjectedStringColumn::Indexed
        );
    }

    #[test]
    fn a_string_one_byte_past_the_limit_overflows() {
        assert_eq!(
            ProjectedStringColumn::of(&"a".repeat(INDEXED_STRING_BYTE_LIMIT + 1)),
            ProjectedStringColumn::Overflow
        );
    }

    /// A two-octet character counts twice: 1024 of them are exactly the limit,
    /// and one more ASCII octet overflows.
    #[test]
    fn the_limit_counts_utf8_octets() {
        let at_limit = "\u{e9}".repeat(1024);
        assert_eq!(ProjectedStringColumn::of(&at_limit), ProjectedStringColumn::Indexed);
        assert_eq!(ProjectedStringColumn::of(&format!("{at_limit}a")), ProjectedStringColumn::Overflow);
    }

    #[test]
    fn each_placement_names_its_column() {
        assert_eq!(ProjectedStringColumn::Indexed.column().to_string(), "value_string");
        assert_eq!(ProjectedStringColumn::Overflow.column().to_string(), "value_long_text");
    }
}
