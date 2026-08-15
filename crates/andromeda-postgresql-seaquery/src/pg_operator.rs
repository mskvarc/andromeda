//! The PostgreSQL infix operators sea-query has no native constructor for.
//!
//! Each operator is a [`BinOper::Custom`] behind a named method, so the token
//! is fixed here and never handed in by a caller. A predicate that tests a
//! jsonb key, matches a POSIX regex or an `ltree` pattern, or tests array
//! containment names the operation, not the punctuation.

use sea_query::{BinOper, ExprTrait, SimpleExpr};

/// The jsonb key-existence operator `?`.
const JSONB_HAS_KEY: &str = "?";

/// The POSIX regular-expression match operator `~`; `ltree` reuses it against an
/// `lquery` operand.
const POSIX_MATCH: &str = "~";

/// The POSIX regular-expression non-match operator `!~`.
const POSIX_NOT_MATCH: &str = "!~";

/// The array-containment operator `@>`.
const ARRAY_CONTAINS: &str = "@>";

/// The array-overlap operator `&&`, true when two arrays share any element.
const ARRAY_OVERLAPS: &str = "&&";

/// The null-aware equality operator `IS NOT DISTINCT FROM`, which treats two
/// NULLs as equal rather than as the unknown `=` yields.
const NOT_DISTINCT_FROM: &str = "IS NOT DISTINCT FROM";

/// The JSONB member-access operator `->`, which yields a JSONB child by key or
/// array index.
const JSON_GET: &str = "->";

/// The JSONB path-extraction-as-text operator `#>>`, which yields the text at a
/// `text[]` path.
const JSON_PATH_TEXT: &str = "#>>";

/// The JSONB member-access-as-text operator `->>`, which yields the text of a
/// child by key or array index.
const JSON_GET_TEXT: &str = "->>";

/// The text concatenation operator `||`.
const TEXT_CONCAT: &str = "||";

/// The `AT TIME ZONE` operator, which reinterprets a `timestamptz` in a named
/// zone (or attaches a zone to a `timestamp`).
const AT_TIME_ZONE: &str = "AT TIME ZONE";

/// The infix PostgreSQL operators a predicate builds through, so the operator
/// token lives in exactly one place.
pub trait PgBinaryOperator {
    /// `self ? key` - whether a jsonb object contains `key` as a top-level key.
    fn jsonb_has_key(self, key: SimpleExpr) -> SimpleExpr;
    /// `self ~ pattern` - whether `self` matches a POSIX regex, or an `ltree`
    /// column matches an `lquery` pattern.
    fn posix_matches(self, pattern: SimpleExpr) -> SimpleExpr;
    /// `self !~ pattern` - whether `self` does not match a POSIX regex.
    fn posix_not_matches(self, pattern: SimpleExpr) -> SimpleExpr;
    /// `self @> element` - whether the array `self` contains `element`.
    fn array_contains(self, element: SimpleExpr) -> SimpleExpr;
    /// `self && other` - whether the arrays `self` and `other` share any element.
    fn array_overlaps(self, other: SimpleExpr) -> SimpleExpr;
    /// `self IS NOT DISTINCT FROM other` - null-aware equality, where two NULLs
    /// compare equal. Clause 4.5.5's default instance carries no `datasetId`, so
    /// its column is NULL, and a plain `=` against NULL yields unknown rather
    /// than matching it.
    fn not_distinct_from(self, other: SimpleExpr) -> SimpleExpr;
    /// `self -> key` - the JSONB child at a top-level `key` or array `index`.
    fn json_get(self, key: SimpleExpr) -> SimpleExpr;
    /// `self #>> path` - the text at a `text[]` `path` into `self`.
    fn json_path_text(self, path: SimpleExpr) -> SimpleExpr;
    /// `self ->> key` - the text of the child at a top-level `key` or array
    /// `index`.
    fn json_get_text(self, key: SimpleExpr) -> SimpleExpr;
    /// `self || other` - concatenate two text values.
    fn concat_text(self, other: SimpleExpr) -> SimpleExpr;
    /// `self AT TIME ZONE zone` - reinterpret a `timestamptz` in `zone`, so a
    /// resolved instant can be rendered in a fixed zone independent of the
    /// session `TimeZone`.
    fn at_time_zone(self, zone: SimpleExpr) -> SimpleExpr;
}

impl PgBinaryOperator for SimpleExpr {
    fn jsonb_has_key(self, key: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(JSONB_HAS_KEY), key)
    }

    fn posix_matches(self, pattern: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(POSIX_MATCH), pattern)
    }

    fn posix_not_matches(self, pattern: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(POSIX_NOT_MATCH), pattern)
    }

    fn array_contains(self, element: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(ARRAY_CONTAINS), element)
    }

    fn array_overlaps(self, other: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(ARRAY_OVERLAPS), other)
    }

    fn not_distinct_from(self, other: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(NOT_DISTINCT_FROM), other)
    }

    fn json_get(self, key: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(JSON_GET), key)
    }

    fn json_path_text(self, path: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(JSON_PATH_TEXT), path)
    }

    fn json_get_text(self, key: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(JSON_GET_TEXT), key)
    }

    fn concat_text(self, other: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(TEXT_CONCAT), other)
    }

    fn at_time_zone(self, zone: SimpleExpr) -> SimpleExpr {
        self.binary(BinOper::Custom(AT_TIME_ZONE), zone)
    }
}

#[cfg(test)]
mod tests {
    use crate::pg_operator::PgBinaryOperator;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn jsonb_has_key_renders_the_question_mark_operator() {
        let sql = render(SimpleExpr::from(Expr::col(Alias::new("payload"))).jsonb_has_key(Expr::val("speed")));
        assert!(sql.contains(" ? "), "{sql}");
    }

    #[test]
    fn posix_match_and_non_match_render_the_regex_operators() {
        assert!(render(SimpleExpr::from(Expr::col(Alias::new("id"))).posix_matches(Expr::val("urn:.*"))).contains(" ~ "));
        assert!(render(SimpleExpr::from(Expr::col(Alias::new("id"))).posix_not_matches(Expr::val("urn:.*"))).contains(" !~ "));
    }

    #[test]
    fn array_contains_renders_the_containment_operator() {
        let sql = render(SimpleExpr::from(Expr::col(Alias::new("types"))).array_contains(Expr::val("Vehicle")));
        assert!(sql.contains(" @> "), "{sql}");
    }

    #[test]
    fn is_not_distinct_from_renders_the_null_aware_equality_operator() {
        let sql = render(SimpleExpr::from(Expr::col(Alias::new("dataset_id"))).not_distinct_from(Expr::col(Alias::new("other"))));
        assert!(sql.contains(" IS NOT DISTINCT FROM "), "{sql}");
    }

    #[test]
    fn the_jsonb_access_operators_render_their_tokens() {
        assert!(render(SimpleExpr::from(Expr::col(Alias::new("payload"))).json_get(Expr::val(0))).contains(" -> "));
        assert!(render(SimpleExpr::from(Expr::col(Alias::new("elem"))).json_path_text(Expr::val("path"))).contains(" #>> "));
        assert!(render(SimpleExpr::from(Expr::col(Alias::new("data"))).json_get_text(Expr::val("type"))).contains(" ->> "));
    }

    #[test]
    fn text_concat_renders_the_double_pipe_operator() {
        let sql = render(SimpleExpr::from(Expr::val("a")).concat_text(Expr::col(Alias::new("suffix"))));
        assert!(sql.contains(" || "), "{sql}");
    }

    #[test]
    fn at_time_zone_renders_the_zone_operator() {
        let sql = render(SimpleExpr::from(Expr::col(Alias::new("observed_at"))).at_time_zone(Expr::val("UTC")));
        assert!(sql.contains(" AT TIME ZONE "), "{sql}");
    }
}
