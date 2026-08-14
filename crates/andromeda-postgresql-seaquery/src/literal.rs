//! Escaping values into single-quoted SQL literals.
//!
//! Most operands reach PostgreSQL as bind parameters and need no escaping. A
//! few cannot: a jsonpath is part of the statement text that the planner parses
//! before any parameter is substituted, and so is the key of a `->'…'` access.
//! Those are interpolated, and an apostrophe in one would otherwise close the
//! literal early — with the rest of the value read as SQL.

/// Escape a value for embedding inside a single-quoted SQL literal.
///
/// Doubling `'` is the whole of it: PostgreSQL's standard-conforming string
/// literals recognise no backslash escapes, so no other character is special.
pub fn escape_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apostrophe_is_doubled() {
        assert_eq!(escape_single_quoted("o'brien"), "o''brien");
    }

    #[test]
    fn a_value_without_apostrophes_is_unchanged() {
        assert_eq!(escape_single_quoted("$[0].\"speed\""), "$[0].\"speed\"");
    }

    #[test]
    fn a_backslash_is_not_special_in_a_standard_conforming_literal() {
        assert_eq!(escape_single_quoted("a\\b"), "a\\b");
    }

    #[test]
    fn every_apostrophe_is_doubled_not_just_the_first() {
        assert_eq!(escape_single_quoted("'a'b'"), "''a''b''");
    }
}
