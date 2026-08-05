//! JSONB access: key existence, and the two escapes a jsonpath needs on its way
//! into a statement.

use crate::literal::escape_single_quoted;
use sea_query::{Expr, SimpleExpr};

/// JSONB: `col ? key` - tests if JSONB object contains key.
pub fn jsonb_key_exists(col: SimpleExpr, key: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ? $2", [col, key])
}

/// Escape one jsonpath object key, which the path itself double-quotes.
pub fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape a jsonpath for embedding in a single-quoted SQL literal.
///
/// Applied unconditionally by every builder that interpolates a jsonpath into
/// `'…'`. An attribute IRI containing an apostrophe would otherwise terminate
/// the literal early; `geoproperty` and `orderBy` both carry request-supplied
/// IRIs, and an absolute IRI passes JSON-LD expansion unchanged.
pub fn escape_jsonpath(jsonpath: &str) -> String {
    escape_single_quoted(jsonpath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn jsonb_key_exists_uses_the_containment_operator() {
        let expr = jsonb_key_exists(Expr::cust("\"payload\"->0"), Expr::val("speed").into());
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
}
