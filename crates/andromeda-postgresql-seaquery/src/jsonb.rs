//! JSONB access: key existence and the two escapes a jsonpath needs on its way
//! into a statement.

use crate::{
    literal::{escape_single_quoted, text_literal},
    pg_operator::PgBinaryOperator,
};
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

/// The `jsonb_path_exists(target, jsonpath)` predicate function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_path_exists"]
struct JsonbPathExists;

/// The `jsonb_path_query_first(target, jsonpath)` extraction function.
#[derive(Iden, Clone, Copy)]
#[iden = "jsonb_path_query_first"]
struct JsonbPathQueryFirst;

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

/// Escape one jsonpath object key, which the path itself double-quotes.
#[must_use]
pub fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape a jsonpath for embedding in a single-quoted SQL literal.
///
/// Applied unconditionally by every builder that interpolates a jsonpath into
/// `'…'`. An attribute IRI containing an apostrophe would otherwise terminate
/// the literal early; `geoproperty` and `orderBy` both carry request-supplied
/// IRIs, and an absolute IRI passes JSON-LD expansion unchanged.
#[must_use]
pub fn escape_jsonpath(jsonpath: &str) -> String {
    escape_single_quoted(jsonpath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

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
    fn a_jsonpath_literal_doubles_apostrophes() {
        assert_eq!(escape_jsonpath("$[0].\"o'brien\""), "$[0].\"o''brien\"");
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
    fn jsonb_path_query_first_inlines_the_jsonpath() {
        let sql = Query::select()
            .expr(jsonb_path_query_first(Expr::col(Alias::new("payload")), "$[0].\"speed\""))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_path_query_first("), "{sql}");
        assert!(sql.contains("'$[0].\"speed\"'"), "{sql}");
    }
}
