//! Writing values into statement text as SQL string literals.
//!
//! Most operands reach PostgreSQL as bind parameters and need no escaping. A
//! few cannot: a jsonpath is part of the statement text that the planner parses
//! before any parameter is substituted, and so is the key of a `->'…'` access,
//! and a utility command takes no parameters at all. Those are written inline,
//! and may carry values derived from a request (an attribute name, a compound
//! member).
//!
//! Every value is written so that PostgreSQL reads it back unchanged whatever
//! `standard_conforming_strings` is. With the setting off, a backslash inside a
//! plain `'…'` literal is an escape character. The broker neither sets nor
//! checks the setting, and a transaction pooler accepts no startup option that
//! could pin it per connection. A value without a backslash is therefore
//! written plain, which reads the same under either setting. A value with a
//! backslash is written as an escape string, `E'…'`, which reads every
//! backslash as an escape under either setting.

use sea_query::{Expr, SimpleExpr};

/// `value` as an SQL string literal that reads back unchanged whatever
/// `standard_conforming_strings` is.
///
/// A value without a backslash is `'…'` with each apostrophe doubled. A value
/// with a backslash is `E'…'` with each backslash and each apostrophe doubled,
/// so a backslash before an apostrophe cannot end the literal early. The one
/// spelling of a literal in statement text, shared by inline expressions and
/// utility commands alike.
pub(crate) fn quoted_text(value: &str) -> String {
    let opening = if value.contains('\\') { "E'" } else { "'" };
    let mut literal = String::with_capacity(value.len() + opening.len() + 1);
    literal.push_str(opening);
    for character in value.chars() {
        if matches!(character, '\'' | '\\') {
            literal.push(character);
        }
        literal.push(character);
    }
    literal.push('\'');
    literal
}

/// An SQL text literal, rendered inline rather than bound.
///
/// For a value that has to be part of the statement text (a jsonpath, the key
/// of a `->` access), or one where binding a parameter would only shift the
/// numbering of the values a caller does bind (a subscription `kind`, an
/// NGSI-LD resource `type`). The value is written by the rule of
/// [`quoted_text`], so it reads back unchanged whatever
/// `standard_conforming_strings` is and may carry values derived from a
/// request.
#[must_use]
pub fn text_literal(value: &str) -> SimpleExpr {
    Expr::cust(quoted_text(value))
}

/// The constant `FALSE`, rendered inline.
///
/// A predicate that no row can satisfy (an attribute IRI no Entity has ever
/// written, a temporal value with no canonical form) renders as this rather
/// than as a bound `false`, so it needs no placeholder and shifts no numbering.
#[must_use]
pub fn false_literal() -> SimpleExpr {
    Expr::cust("FALSE")
}

/// The constant `TRUE`, rendered inline: a predicate every row satisfies,
/// rendered without a placeholder for the reason [`false_literal`] is.
#[must_use]
pub fn true_literal() -> SimpleExpr {
    Expr::cust("TRUE")
}

/// An integer rendered inline rather than bound.
///
/// For a statement-level constant an `EXISTS` probe's `SELECT 1` marker is the
/// canonical example, where binding it would only add a placeholder to every
/// probe and shift the numbering of the values that carry meaning. The value is
/// a broker-minted integer, never caller-supplied data.
#[must_use]
pub fn int_literal(value: i64) -> SimpleExpr {
    Expr::cust(value.to_string())
}

#[cfg(test)]
mod tests {
    use crate::literal::{quoted_text, text_literal};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_value_without_a_backslash_is_a_plain_literal_with_its_apostrophe_doubled() {
        assert_eq!(quoted_text("o'brien"), "'o''brien'");
    }

    #[test]
    fn every_apostrophe_of_a_plain_literal_is_doubled_not_just_the_first() {
        assert_eq!(quoted_text("'a'b'"), "'''a''b'''");
    }

    #[test]
    fn a_value_without_apostrophes_or_backslashes_is_quoted_as_written() {
        assert_eq!(quoted_text("$[0].\"speed\""), "'$[0].\"speed\"'");
    }

    #[test]
    fn an_empty_value_is_an_empty_plain_literal() {
        assert_eq!(quoted_text(""), "''");
    }

    #[test]
    fn a_value_with_a_backslash_is_an_escape_string_with_backslashes_and_apostrophes_doubled() {
        assert_eq!(quoted_text(r"a\b'c"), r"E'a\\b''c'");
    }

    #[test]
    fn a_backslash_before_an_apostrophe_is_doubled_so_it_cannot_end_the_literal() {
        assert_eq!(quoted_text(r"\'"), r"E'\\'''");
    }

    #[test]
    fn a_jsonpath_key_escaped_for_jsonpath_is_an_escape_string() {
        assert_eq!(quoted_text(r#"$."a\"b""#), r#"E'$."a\\"b"'"#);
    }

    #[test]
    fn a_text_literal_is_rendered_inline_not_bound() {
        let (sql, values) = Query::select().expr(text_literal("entity")).to_owned().build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT 'entity'");
        assert_eq!(values.0.len(), 0, "a literal is rendered inline, not bound: {sql}");
    }

    #[test]
    fn a_text_literal_holding_a_backslash_is_rendered_as_an_escape_string() {
        let (sql, values) = Query::select().expr(text_literal(r"a\b'c")).to_owned().build(PostgresQueryBuilder);
        assert_eq!(sql, r"SELECT E'a\\b''c'");
        assert_eq!(values.0.len(), 0, "a literal is rendered inline, not bound: {sql}");
    }

    #[test]
    fn a_statement_inlining_an_escape_string_renders_it_unchanged() {
        let sql = Query::select().expr(text_literal(r"\'")).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, r"SELECT E'\\'''");
    }
}
