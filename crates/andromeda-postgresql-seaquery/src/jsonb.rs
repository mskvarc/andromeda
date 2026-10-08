//! JSONB access: key existence and the escape a jsonpath object key needs on
//! its way into a statement.

use crate::{literal::text_literal, pg_operator::PgBinaryOperator};
use sea_query::{Func, FunctionCall, Iden, SimpleExpr};

/// The `jsonb_array_elements_text` set-returning function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_array_elements_text"]
struct JsonbArrayElementsText;

/// The `jsonb_array_elements` set-returning function, whose one output column is
/// `value`.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_array_elements"]
struct JsonbArrayElements;

/// The `jsonb_each` set-returning function, whose output columns are `key` and
/// `value`.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_each"]
struct JsonbEach;

/// The `jsonb_object_keys` set-returning function, whose one output column is
/// the key name.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_object_keys"]
struct JsonbObjectKeys;

/// The `jsonb_typeof` function, which names the JSON type of a value as text.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_typeof"]
struct JsonbTypeof;

/// The `jsonb_exists(target, key)` function, the function spelling of the `?`
/// key-existence operator.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_exists"]
struct JsonbExists;

/// The `jsonb_array_length(array)` function, the element count of a JSONB array.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_array_length"]
struct JsonbArrayLength;

/// The `jsonb_build_array(VARIADIC "any")` constructor.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_build_array"]
struct JsonbBuildArray;

/// The `jsonb_build_object(VARIADIC "any")` constructor.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_build_object"]
struct JsonbBuildObject;

/// The `to_jsonb(value)` conversion of any SQL value to its JSONB form.
#[derive(Iden, Clone, Copy)]
#[iden = "to_jsonb"]
struct ToJsonb;

/// The `jsonb_strip_nulls(value)` function, which drops every object member
/// whose value is JSON null, at every depth.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_strip_nulls"]
struct JsonbStripNulls;

/// The `jsonb_path_exists(target, jsonpath)` predicate function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_path_exists"]
struct JsonbPathExists;

/// The `jsonb_path_query_first(target, jsonpath)` extraction function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_path_query_first"]
struct JsonbPathQueryFirst;

/// The `jsonb_path_query(target, jsonpath)` set-returning function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_path_query"]
struct JsonbPathQuery;

/// The `jsonb_path_query_array(target, jsonpath)` function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_path_query_array"]
struct JsonbPathQueryArray;

/// JSONB: `col ? key` - tests if JSONB object contains key.
#[must_use]
pub fn jsonb_key_exists(col: SimpleExpr, key: SimpleExpr) -> SimpleExpr {
    col.jsonb_has_key(key)
}

/// JSONB: `jsonb_array_elements_text(array)` - the set of text elements of a
/// JSONB array, one row each.
///
/// A set-returning function returned as a [`FunctionCall`] so it stands either
/// in a `FROM`/`JOIN` position where a set is expanded to rows, or — converted
/// to an expression — inside a subquery whose result an `ARRAY(…)` collects into
/// a Postgres array, which is how a JSONB array of strings becomes a `text[]`
/// column. Its one output column is the text value.
#[must_use]
pub fn jsonb_array_elements_text(array: SimpleExpr) -> FunctionCall {
    Func::cust(JsonbArrayElementsText).args([array])
}

/// JSONB: `jsonb_exists(target, key)` - the function spelling of `target ? key`,
/// whether the JSONB object `target` contains `key` as a top-level key.
///
/// The function form composes where the `?` operator's precedence is awkward —
/// as a negated conjunct `NOT jsonb_exists(...)` or beside a set-returning call.
#[must_use]
pub fn jsonb_exists(target: SimpleExpr, key: SimpleExpr) -> SimpleExpr {
    Func::cust(JsonbExists).args([target, key]).into()
}

/// JSONB: `jsonb_array_elements(array)` as a table source, whose one output
/// column is `value`.
///
/// Returned as a [`FunctionCall`] so it stands in a `FROM`/`JOIN LATERAL`
/// position where a set is expanded to rows, rather than in a projection.
#[must_use]
pub fn jsonb_array_elements(array: SimpleExpr) -> FunctionCall {
    Func::cust(JsonbArrayElements).args([array])
}

/// JSONB: `jsonb_each(object)` as a table source, whose output columns are `key`
/// and `value`.
#[must_use]
pub fn jsonb_each(object: SimpleExpr) -> FunctionCall {
    Func::cust(JsonbEach).args([object])
}

/// JSONB: `jsonb_object_keys(object)` as a set-returning source, whose one
/// output column is each top-level key name of `object`.
#[must_use]
pub fn jsonb_object_keys(object: SimpleExpr) -> FunctionCall {
    Func::cust(JsonbObjectKeys).args([object])
}

/// JSONB: `jsonb_typeof(value)` - the JSON type of `value` as text (`object`,
/// `array`, `string`, `number`, `boolean`, `null`).
#[must_use]
pub fn jsonb_typeof(value: SimpleExpr) -> SimpleExpr {
    Func::cust(JsonbTypeof).args([value]).into()
}

/// JSONB: `jsonb_build_array(element)` - a JSONB array holding one element.
///
/// Used to compare a Query Term against a target the NGSI-LD information model
/// stores as a structured value (clause 4.9): the comparison is JSONB
/// containment, so the term is built into the same shape the stored value has.
/// Building it here rather than as JSON text keeps each operand bound as its
/// own PostgreSQL type and hand-rolls no JSON escaping.
#[must_use]
pub fn jsonb_build_array(element: SimpleExpr) -> SimpleExpr {
    Func::cust(JsonbBuildArray).args([element]).into()
}

/// JSONB: `jsonb_build_object(key, value, ...)` - a JSONB object, taking its
/// arguments already paired.
#[must_use]
pub fn jsonb_build_object<Members: IntoIterator<Item = SimpleExpr>>(members: Members) -> SimpleExpr {
    Func::cust(JsonbBuildObject).args(members).into()
}

/// JSONB: `to_jsonb(value)` - any SQL value as the JSONB value it converts to:
/// a number as a JSON number, text as a JSON string, a timestamp as its ISO 8601
/// text, and SQL NULL as SQL NULL.
#[must_use]
pub fn to_jsonb(value: SimpleExpr) -> SimpleExpr {
    Func::cust(ToJsonb).args([value]).into()
}

/// JSONB: `jsonb_strip_nulls(value)` - `value` with every object member whose
/// value is JSON null removed, at every depth, so a document assembled from
/// optional columns carries only the members that hold something.
#[must_use]
pub fn jsonb_strip_nulls(value: SimpleExpr) -> SimpleExpr {
    Func::cust(JsonbStripNulls).args([value]).into()
}

/// JSONB: `jsonb_array_length(array)` - the number of elements in a JSONB array.
///
/// Used by type-aware aggregation (clause 4.5.19.1), where an array-valued
/// instance counts as its length so `avg`/`sum` mean something for it.
#[must_use]
pub fn jsonb_array_length(array: SimpleExpr) -> SimpleExpr {
    Func::cust(JsonbArrayLength).args([array]).into()
}

/// JSONB: `jsonb_path_exists(target, 'jsonpath')` - whether the `jsonpath`
/// matches anything in `target`.
///
/// The jsonpath is part of the statement text the planner parses before any
/// parameter is substituted, so it is inlined as a single-quoted literal — the
/// key escaping the path itself needs is already applied by its builder, and
/// [`text_literal`] adds the SQL escaping the surrounding literal needs.
#[must_use]
pub fn jsonb_path_exists(target: SimpleExpr, jsonpath: &str) -> SimpleExpr {
    Func::cust(JsonbPathExists).args([target, text_literal(jsonpath)]).into()
}

/// JSONB: `jsonb_path_query_first(target, 'jsonpath')` - the first value in
/// `target` the `jsonpath` matches, or JSON null when it matches nothing.
///
/// The jsonpath is inlined for the same reason [`jsonb_path_exists`] inlines it.
#[must_use]
pub fn jsonb_path_query_first(target: SimpleExpr, jsonpath: &str) -> SimpleExpr {
    Func::cust(JsonbPathQueryFirst).args([target, text_literal(jsonpath)]).into()
}

/// JSONB: `jsonb_path_query(target, 'jsonpath')` as a table source, one row
/// per value in `target` the `jsonpath` matches.
///
/// Returned as a [`FunctionCall`] so it stands in a `FROM` position; its one
/// output column carries the name of the alias it is given there. The jsonpath
/// is inlined for the same reason [`jsonb_path_exists`] inlines it.
#[must_use]
pub fn jsonb_path_query(target: SimpleExpr, jsonpath: &str) -> FunctionCall {
    Func::cust(JsonbPathQuery).args([target, text_literal(jsonpath)])
}

/// JSONB: `jsonb_path_query_array(target, 'jsonpath')` - every value in
/// `target` the `jsonpath` matches, gathered into one JSONB array, which is
/// empty when it matches nothing.
///
/// The jsonpath is inlined for the same reason [`jsonb_path_exists`] inlines it.
#[must_use]
pub fn jsonb_path_query_array(target: SimpleExpr, jsonpath: &str) -> SimpleExpr {
    Func::cust(JsonbPathQueryArray).args([target, text_literal(jsonpath)]).into()
}

/// Escape one jsonpath object key, which the path itself double-quotes.
#[must_use]
pub fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn to_jsonb_and_strip_nulls_render_as_the_functions_they_name() {
        let sql = Query::select()
            .expr(jsonb_strip_nulls(to_jsonb(Expr::col((Alias::new("h"), Alias::new("value_number"))))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT jsonb_strip_nulls(to_jsonb("h"."value_number"))"#);
    }

    #[test]
    fn jsonb_key_exists_uses_the_containment_operator() {
        let expr = jsonb_key_exists(Expr::cust("\"payload\"->0"), Expr::val("speed"));
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" ? "), "{sql}");
    }

    #[test]
    fn a_jsonpath_key_escapes_backslashes_and_double_quotes() {
        assert_eq!(escape_jsonpath_key("a\\b\"c"), "a\\\\b\\\"c");
    }

    #[test]
    fn jsonb_path_exists_inlines_and_escapes_the_jsonpath() {
        let sql = Query::select()
            .expr(jsonb_path_exists(Expr::col(Alias::new("payload")), "$[0].\"o'brien\""))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_path_exists("), "{sql}");
        assert!(sql.contains("'$[0].\"o''brien\"'"), "the jsonpath is inlined and its apostrophe doubled: {sql}");
    }

    #[test]
    fn jsonb_path_query_array_inlines_the_jsonpath() {
        let sql = Query::select()
            .expr(jsonb_path_query_array(Expr::col(Alias::new("payload")), "$[0].\"name\"[*]"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_path_query_array(\"payload\", '$[0].\"name\"[*]')"), "{sql}");
    }

    #[test]
    fn jsonb_path_query_is_a_table_source_with_the_jsonpath_inlined() {
        let sql = Query::select()
            .expr(Expr::val(1))
            .from_function(jsonb_path_query(Expr::col(Alias::new("payload")), "$[0].\"name\"[*]"), Alias::new("node"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("FROM jsonb_path_query(\"payload\", '$[0].\"name\"[*]') AS \"node\""), "{sql}");
    }

    #[test]
    fn jsonb_path_query_first_inlines_the_jsonpath() {
        let sql = Query::select()
            .expr(jsonb_path_query_first(Expr::col(Alias::new("payload")), "$[0].\"speed\""))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_path_query_first("), "{sql}");
        assert!(sql.contains("'$[0].\"speed\"'"), "{sql}");
    }
}
