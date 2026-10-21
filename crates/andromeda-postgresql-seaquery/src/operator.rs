//! The generic SQL operators sea-query has no native form for: array
//! containment in both directions, POSIX regex matching and extraction, the
//! presence-and-value conjunction, the total negation of a predicate over
//! nullable columns, and the NULL a sentinel value stands for.

use crate::{literal::false_literal, pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::{Expr, ExprTrait, Func, Iden, SimpleExpr};

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

/// Whether every dictionary key of `array_col` is one of `keys`:
/// `array_col <@ ARRAY[k, …]::integer[]`, the keys rendered inline.
///
/// Inlined for the reason [`array_contains_key`] gives, and cast so an empty
/// key list still names the element type: `ARRAY[]` alone has none, and
/// PostgreSQL refuses it. An empty `array_col` is contained by any array,
/// the empty one included.
#[must_use]
pub fn array_contained_by_keys(array_col: SimpleExpr, keys: &[i32]) -> SimpleExpr {
    array_col.array_contained_by(key_array(keys))
}

/// `ARRAY[k, …]::integer[]`, dictionary keys rendered inline.
///
/// Inlined for the reason [`array_contains_key`] gives: a key array is part of
/// the statement's shape, which a constant keeps visible to the planner, and an
/// immutable function over it folds at planning. Cast so an empty list still
/// names the element type.
#[must_use]
pub fn key_array(keys: &[i32]) -> SimpleExpr {
    let keys = keys.iter().map(i32::to_string).collect::<Vec<_>>().join(", ");
    Expr::cust(format!("ARRAY[{keys}]::integer[]"))
}

/// `ARRAY[k, …]::bigint[]`, hash keys rendered inline.
///
/// Inlined for the reason [`array_contains_key`] gives: the planner reads a
/// `bigint[]` column's most-common-element statistics for the very keys a
/// constant names, and prices a bound array at the operator's default
/// selectivity. A hash key is computed by the broker from a value, never
/// caller-supplied text, which is what makes inlining it safe. Cast so an
/// empty list still names the element type.
#[must_use]
pub fn hash_key_array(keys: &[i64]) -> SimpleExpr {
    let keys = keys.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
    Expr::cust(format!("ARRAY[{keys}]::bigint[]"))
}

/// Regex: `col ~ pattern` - tests if column matches POSIX regex.
#[must_use]
pub fn regex_match(col: SimpleExpr, pattern: SimpleExpr) -> SimpleExpr {
    col.posix_matches(pattern)
}

/// The function a POSIX regex extraction is spelled with.
#[derive(Iden, Clone, Copy)]
enum TextFunction {
    /// `substring(text, pattern)`.
    #[iden = "substring"]
    Substring,
}

/// Regex extraction: `substring(text, pattern)`, the part of `text` the first
/// parenthesised subexpression of the POSIX `pattern` matches, or the whole
/// match when the pattern has none. NULL when `text` does not match.
#[must_use]
pub fn regex_substring(text: SimpleExpr, pattern: SimpleExpr) -> SimpleExpr {
    Func::cust(TextFunction::Substring).args([text, pattern]).into()
}

/// The function a sentinel value is mapped to NULL with.
#[derive(Iden, Clone, Copy)]
enum ConditionalFunction {
    /// `nullif(value, sentinel)`.
    #[iden = "nullif"]
    NullIf,
}

/// `nullif(value, sentinel)`: NULL when `value` equals `sentinel`, `value`
/// otherwise.
#[must_use]
pub fn null_if(value: SimpleExpr, sentinel: SimpleExpr) -> SimpleExpr {
    Func::cust(ConditionalFunction::NullIf).args([value, sentinel]).into()
}

/// The total negation of a predicate: `NOT COALESCE(predicate, FALSE)`.
///
/// Plain `NOT` propagates the unknown a comparison against a NULL yields, and
/// the enclosing `WHERE` then drops the row — so `NOT` is not the complement of
/// a predicate over nullable columns, only of one over rows where every operand
/// is present. Wrapping the predicate first makes the negation total: a row the
/// predicate could not be evaluated for satisfies the negation.
///
/// `COALESCE` rather than the equivalent `IS NOT TRUE`, whose right-hand side is
/// grammar rather than an expression: sea-query parenthesises every operand it
/// renders, and PostgreSQL rejects `… IS NOT (TRUE)`.
#[must_use]
pub fn negate_total(predicate: SimpleExpr) -> SimpleExpr {
    Func::coalesce([predicate, false_literal()]).not()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_sentinel_renders_as_nullif() {
        let expr = null_if(Expr::col("limit"), Expr::val("-1"));
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT nullif("limit", '-1')"#);
    }

    #[test]
    fn regex_extraction_renders_the_two_argument_substring() {
        let expr = regex_substring(Expr::col("nspname"), Expr::val("^snap_([1-9][0-9]*)$"));
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT substring("nspname", '^snap_([1-9][0-9]*)$')"#);
    }

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
        let expr = array_contains(Expr::col("entity_type_name_keys"), Expr::val(7_i32), PostgresType::Integer);
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"entity_type_name_keys\" @> ARRAY(SELECT 7::integer)"), "{sql}");
    }

    /// A dictionary key is inlined so the planner can estimate the containment
    /// from the column's element statistics; a bound key would be planned at
    /// the operator's default selectivity.
    #[test]
    fn dictionary_key_membership_renders_the_key_as_an_inline_array_literal() {
        let expr = array_contains_key(Expr::col("entity_type_name_keys"), 7);
        let (sql, values) = Query::select().expr(expr).to_owned().build(PostgresQueryBuilder);
        assert!(sql.contains("\"entity_type_name_keys\" @> (ARRAY[7])"), "{sql}");
        assert!(values.0.is_empty(), "the key must not be bound: {sql}");
    }

    /// The keys are inlined with the element type, so the list the planner
    /// reads is a typed constant, the empty one included.
    #[test]
    fn a_key_subset_test_renders_an_inline_typed_array() {
        let (sql, values) = Query::select()
            .expr(array_contained_by_keys(Expr::col("attribute_name_keys"), &[3, 5]))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("\"attribute_name_keys\" <@ (ARRAY[3, 5]::integer[])"), "{sql}");
        assert!(values.0.is_empty(), "the keys must not be bound: {sql}");

        let sql = Query::select()
            .expr(array_contained_by_keys(Expr::col("attribute_name_keys"), &[]))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("<@ (ARRAY[]::integer[])"), "{sql}");
    }

    /// A hash key spans the whole signed 64-bit range, so the array is typed
    /// `bigint[]` and a negative key is written as one.
    #[test]
    fn hash_keys_render_as_an_inline_bigint_array() {
        let (sql, values) = Query::select()
            .expr(Expr::col("sub_eq_keys").array_overlaps(hash_key_array(&[i64::MIN, 7])))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("\"sub_eq_keys\" && (ARRAY[-9223372036854775808, 7]::bigint[])"), "{sql}");
        assert!(values.0.is_empty(), "the keys must not be bound: {sql}");
    }

    #[test]
    fn a_value_test_is_always_conjoined_with_presence() {
        let presence = Expr::col("stored").is_not_null();
        let value = Expr::col("stored").eq("x");
        let sql = Query::select().expr(present_and(presence, value)).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" IS NOT NULL AND "), "presence is conjoined, never disjoined: {sql}");
    }

    /// A row the predicate cannot be evaluated for satisfies the negation,
    /// which plain `NOT` would drop instead.
    #[test]
    fn a_total_negation_wraps_the_predicate_before_negating_it() {
        let predicate = Expr::col("stored").eq("x");
        let sql = Query::select().expr(negate_total(predicate)).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("NOT COALESCE("), "{sql}");
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("IS NOT (TRUE)"), "PostgreSQL rejects a parenthesised IS NOT operand: {sql}");
    }

    #[test]
    fn regex_match_generates_the_posix_operator() {
        let expr = regex_match(Expr::col("id"), Expr::val("urn:.*"));
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" ~ "), "{sql}");
    }
}
