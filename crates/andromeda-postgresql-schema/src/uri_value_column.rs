//! Which of the two URI columns of `current_attributes` a Relationship
//! object is held in.
//!
//! `idx_ca_value_uri` keys on `value_uri`, and a B-tree refuses an entry past
//! 2704 bytes (SQLSTATE 54000). Clause 4.6.2 sets no length on a URI, so a
//! Relationship object longer than [`INDEXED_URI_BYTE_LIMIT`] is held in
//! `value_long_uri` instead. The migration's two length checks make that
//! placement canonical: every URI has exactly one column it may sit in. The
//! writer places a value through [`UriValueColumn::of`] and the `q`
//! compiler routes an equality through the same function, which is what makes
//! reading only the operand's own column exact.
//!
//! The bound equals the one every stored identifier is held to, so an object
//! naming a local Entity always sits in `value_uri`; an object in
//! `value_long_uri` names no local Entity.

use crate::current::CurrentAttributes;

/// The greatest length, in UTF-8 octets, a URI held in `value_uri` may have.
///
/// 1024 leaves the `(attribute_name_key, value_uri)` entry of
/// `idx_ca_value_uri` well inside the 2704 bytes a B-tree entry may take. A
/// unit test in `migration_set` pins the literal the `current_attributes` migration states to this
/// constant, and another pins it to the identifier bound.
pub const INDEXED_URI_BYTE_LIMIT: usize = 1024;

/// The column one Relationship object is held in.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UriValueColumn {
    /// `value_uri`, keyed by a B-tree: at most [`INDEXED_URI_BYTE_LIMIT`]
    /// octets.
    Indexed,
    /// `value_long_uri`, past what the B-tree on `value_uri` holds.
    Overflow,
}

impl UriValueColumn {
    /// The column `uri` is held in.
    #[must_use]
    pub const fn of(uri: &str) -> UriValueColumn {
        if uri.len() <= INDEXED_URI_BYTE_LIMIT {
            UriValueColumn::Indexed
        } else {
            UriValueColumn::Overflow
        }
    }

    /// The column this placement names.
    #[must_use]
    pub const fn column(self) -> CurrentAttributes {
        match self {
            UriValueColumn::Indexed => CurrentAttributes::ValueUri,
            UriValueColumn::Overflow => CurrentAttributes::ValueLongUri,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::uri_value_column::{INDEXED_URI_BYTE_LIMIT, UriValueColumn};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    /// A URN whose spelling is exactly `length` octets long.
    fn urn_of_length(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Building:";
        format!("{prefix}{}", "a".repeat(length - prefix.len()))
    }

    #[test]
    fn a_uri_of_exactly_the_limit_is_indexed() {
        assert_eq!(UriValueColumn::of(&urn_of_length(INDEXED_URI_BYTE_LIMIT)), UriValueColumn::Indexed);
    }

    #[test]
    fn a_uri_one_byte_past_the_limit_overflows() {
        assert_eq!(UriValueColumn::of(&urn_of_length(INDEXED_URI_BYTE_LIMIT + 1)), UriValueColumn::Overflow);
    }

    #[test]
    fn a_uri_far_past_the_limit_overflows() {
        assert_eq!(UriValueColumn::of(&urn_of_length(6000)), UriValueColumn::Overflow);
    }

    /// A two-octet character counts twice: an IRI (clause 4.6.2 NOTE 2) whose
    /// non-ASCII characters bring it to exactly the limit is indexed, and one
    /// more ASCII octet overflows although it is far shorter in characters.
    #[test]
    fn the_limit_counts_utf8_octets() {
        let prefix = "urn:ngsi-ld:Ciudad:";
        let at_limit = format!("{prefix}x{}", "\u{e9}".repeat((INDEXED_URI_BYTE_LIMIT - prefix.len() - 1) / 2));
        assert_eq!(at_limit.len(), INDEXED_URI_BYTE_LIMIT, "the fixture must sit exactly at the bound");
        assert!(at_limit.chars().count() < INDEXED_URI_BYTE_LIMIT);
        assert_eq!(UriValueColumn::of(&at_limit), UriValueColumn::Indexed);
        assert_eq!(UriValueColumn::of(&format!("{at_limit}a")), UriValueColumn::Overflow);
    }

    #[test]
    fn each_placement_names_its_column() {
        assert_eq!(UriValueColumn::Indexed.column().to_string(), "value_uri");
        assert_eq!(UriValueColumn::Overflow.column().to_string(), "value_long_uri");
    }
}
