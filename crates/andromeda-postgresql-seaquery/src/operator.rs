//! The generic SQL operators sea-query has no native form for: array
//! containment, POSIX regex, and the presence-and-value conjunction.

use crate::{pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::{Expr, ExprTrait, Iden, SimpleExpr};

/// Combine a presence test with a value test.
///
/// Presence is conjoined in both polarities, so a value test cannot silently
/// hold for a row that lacks the value entirely. Every rendering that tests a
/// value builds its predicate here, which is what stops one of them disjoining
/// presence instead.
#[must_use]
pub fn present_and(presence: SimpleExpr, value: SimpleExpr) -> SimpleExpr {
    presence.and(value)
}

/// Array membership: `array_col @> ARRAY(SELECT value::element_type)`.
///
/// Containment rather than `value = ANY(array_col)`, which states the same
/// thing and is the more obvious spelling: GIN `array_ops` serves `@>`, `&&`
/// and `<@` and does not serve `= ANY`, so the equality form makes every type
/// selection a sequential scan no matter how selective the type is.
///
/// The single-element array is built by a scalar subquery rather than as
/// `ARRAY[$1]`: sea-query's custom-expression tokeniser reads a bracketed span
/// as a quoted identifier and copies it out verbatim, which would leave the
/// placeholder unsubstituted and silently aliased onto another bind. The
/// subquery form plans as an `InitPlan` whose result the index scan takes as
/// its condition, so the access path is the same one a literal array gets.
///
/// The element is cast to `element_type` so the single-element array has the
/// column's element type whatever the bound value's inferred type is: an
/// `integer[]` dictionary-key column and a `text[]` column are both served.
#[must_use]
pub fn array_contains(array_col: SimpleExpr, value: SimpleExpr, element_type: PostgresType) -> SimpleExpr {
    Expr::cust_with_exprs(format!("$1 @> ARRAY(SELECT $2::{})", Iden::to_string(&element_type)), [array_col, value])
}

/// Membership of one dictionary key: `array_col @> ARRAY[key]`, the key
/// rendered inline as an integer literal.
///
/// The literal form exists for the planner. A key that reaches the statement
/// as a bind parameter, or through the scalar subquery [`array_contains`]
/// builds, is opaque at planning time: the containment selectivity falls back
/// to the operator's default, every dictionary key looks equally common, and a
/// rare key's query is planned as an ordered walk of the whole relation instead
/// of a GIN probe. Written as a constant the planner reads the column's
/// most-common-element statistics for that very key and picks the probe. A
/// dictionary key is broker-minted (`attribute_names.id`,
/// `entity_type_names.id`) and never caller-supplied text, which is what makes
/// inlining it safe; the element type of `ARRAY[<integer>]` is `integer[]`, the
/// type of every dictionary-key column.
#[must_use]
pub fn array_contains_key(array_col: SimpleExpr, key: i32) -> SimpleExpr {
    array_col.array_contains(Expr::cust(format!("ARRAY[{key}]")))
}

/// Regex: `col ~ pattern` - tests if column matches POSIX regex.
#[must_use]
pub fn regex_match(col: SimpleExpr, pattern: SimpleExpr) -> SimpleExpr {
    col.posix_matches(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    /// GIN `array_ops` has an operator class for containment and none for
    /// `= ANY`, so the rendered leaf must be the containment form.
    #[test]
    fn array_membership_renders_as_containment_over_a_single_element_array() {
        let expr = array_contains(Expr::col("types"), Expr::val("type_iri"), PostgresType::Text);
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("@> ARRAY(SELECT "), "{sql}");
        assert!(!sql.contains("= ANY"), "{sql}");
        assert!(
            sql.contains("'type_iri'::text)"),
            "the value must survive substitution and carry the element cast: {sql}"
        );
    }

    /// A dictionary-key column is `integer[]`, and the single-element array
    /// has to carry that element type for the GIN operator class to apply.
    #[test]
    fn array_membership_casts_the_element_to_the_columns_element_type() {
        let expr = array_contains(Expr::col("type_ids"), Expr::val(7_i32), PostgresType::Integer);
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"type_ids\" @> ARRAY(SELECT 7::integer)"), "{sql}");
    }

    /// A dictionary key is inlined so the planner can estimate the containment
    /// from the column's element statistics; a bound key would be planned at
    /// the operator's default selectivity.
    #[test]
    fn dictionary_key_membership_renders_the_key_as_an_inline_array_literal() {
        let expr = array_contains_key(Expr::col("type_ids"), 7);
        let (sql, values) = Query::select().expr(expr).to_owned().build(PostgresQueryBuilder);
        assert!(sql.contains("\"type_ids\" @> (ARRAY[7])"), "{sql}");
        assert!(values.0.is_empty(), "the key must not be bound: {sql}");
    }

    #[test]
    fn a_value_test_is_always_conjoined_with_presence() {
        let presence = Expr::col("stored").is_not_null();
        let value = Expr::col("stored").eq("x");
        let sql = Query::select().expr(present_and(presence, value)).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" IS NOT NULL AND "), "presence is conjoined, never disjoined: {sql}");
    }

    #[test]
    fn regex_match_generates_the_posix_operator() {
        let expr = regex_match(Expr::col("id"), Expr::val("urn:.*"));
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" ~ "), "{sql}");
    }
}
