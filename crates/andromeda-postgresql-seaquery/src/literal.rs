//! Escaping values into single-quoted SQL literals.
//!
//! Most operands reach PostgreSQL as bind parameters and need no escaping. A
//! few cannot: a jsonpath is part of the statement text that the planner parses
//! before any parameter is substituted, and so is the key of a `->'…'` access.
//! Those are interpolated, and an apostrophe in one would otherwise close the
//! literal early — with the rest of the value read as SQL.

use sea_query::{Expr, SimpleExpr};

/// Escape a value for embedding inside a single-quoted SQL literal.
///
/// Doubling `'` is the whole of it: PostgreSQL's standard-conforming string
/// literals recognise no backslash escapes, so no other character is special.
#[must_use]
pub fn escape_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

/// A single-quoted SQL text literal, rendered inline rather than bound.
///
/// For a fixed vocabulary token a statement writes directly — a subscription
/// `kind`, an NGSI-LD resource `type` — where binding a parameter would only
/// shift the numbering of the values a caller does bind. The value is escaped,
/// so an apostrophe cannot terminate the literal early; it is never a channel
/// for caller-supplied data, which always binds.
#[must_use]
pub fn text_literal(value: &str) -> SimpleExpr {
    Expr::cust(format!("'{}'", escape_single_quoted(value)))
}

/// The constant `FALSE`, rendered inline.
///
/// A predicate that no row can satisfy — an attribute IRI no Entity has ever
/// written, a temporal value with no canonical form — renders as this rather
/// than as a bound `false`, so it needs no placeholder and shifts no numbering.
#[must_use]
pub fn false_literal() -> SimpleExpr {
    Expr::cust("FALSE")
}

/// An integer rendered inline rather than bound.
///
/// For a statement-level constant an `EXISTS` probe's `SELECT 1` marker is the
/// canonical example — where binding it would only add a placeholder to every
/// probe and shift the numbering of the values that carry meaning. The value is
/// a broker-minted integer, never caller-supplied data.
#[must_use]
pub fn int_literal(value: i64) -> SimpleExpr {
    Expr::cust(value.to_string())
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

    #[test]
    fn a_text_literal_is_single_quoted_and_escaped_inline() {
        use crate::literal::text_literal;
        use sea_query::{PostgresQueryBuilder, Query};
        let (sql, values) = Query::select().expr(text_literal("entity")).to_owned().build(PostgresQueryBuilder);
        assert!(sql.contains("'entity'"), "{sql}");
        assert_eq!(values.0.len(), 0, "a literal is rendered inline, not bound: {sql}");
    }
}
