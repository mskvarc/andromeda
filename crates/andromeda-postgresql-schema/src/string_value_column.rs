//! Which of the two string columns of `current_attributes` a String
//! value is held in.
//!
//! A B-tree refuses an entry past 2704 bytes (SQLSTATE 54000), and
//! `idx_ca_value_string` keys on `value_string`, so a String longer than
//! [`INDEXED_STRING_BYTE_LIMIT`] is held in `value_long_text` instead. The
//! migration's two length checks make that placement canonical: every string
//! has exactly one column it may sit in. The writer places a value through
//! [`StringValueColumn::of`] and the `q` compiler routes an equality
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
pub enum StringValueColumn {
    /// `value_string`, keyed by a B-tree: at most [`INDEXED_STRING_BYTE_LIMIT`]
    /// octets.
    Indexed,
    /// `value_long_text`, past what a B-tree entry may hold.
    Overflow,
}

impl StringValueColumn {
    /// The column `text` is held in.
    #[must_use]
    pub const fn of(text: &str) -> StringValueColumn {
        if text.len() <= INDEXED_STRING_BYTE_LIMIT {
            StringValueColumn::Indexed
        } else {
            StringValueColumn::Overflow
        }
    }

    /// The column this placement names.
    #[must_use]
    pub const fn column(self) -> CurrentAttributes {
        match self {
            StringValueColumn::Indexed => CurrentAttributes::ValueString,
            StringValueColumn::Overflow => CurrentAttributes::ValueLongText,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::string_value_column::{INDEXED_STRING_BYTE_LIMIT, StringValueColumn};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn a_string_of_exactly_the_limit_is_indexed() {
        assert_eq!(StringValueColumn::of(&"a".repeat(INDEXED_STRING_BYTE_LIMIT)), StringValueColumn::Indexed);
    }

    #[test]
    fn a_string_one_byte_past_the_limit_overflows() {
        assert_eq!(StringValueColumn::of(&"a".repeat(INDEXED_STRING_BYTE_LIMIT + 1)), StringValueColumn::Overflow);
    }

    /// A two-octet character counts twice: 1024 of them are exactly the limit,
    /// and one more ASCII octet overflows.
    #[test]
    fn the_limit_counts_utf8_octets() {
        let at_limit = "\u{e9}".repeat(1024);
        assert_eq!(StringValueColumn::of(&at_limit), StringValueColumn::Indexed);
        assert_eq!(StringValueColumn::of(&format!("{at_limit}a")), StringValueColumn::Overflow);
    }

    #[test]
    fn each_placement_names_its_column() {
        assert_eq!(StringValueColumn::Indexed.column().to_string(), "value_string");
        assert_eq!(StringValueColumn::Overflow.column().to_string(), "value_long_text");
    }
}
