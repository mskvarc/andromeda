//! JSONB access: key existence, and the two escapes a jsonpath needs on its way
//! into a statement.

use crate::literal::escape_single_quoted;
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use sea_query::{Expr, SimpleExpr};

/// A comparison operand and the cast the comparison needs around it.
///
/// A value read out of JSONB with `#>>` arrives as text, so both sides have to
/// be cast to the same type before they are ordered. Without the cast a numeric
/// comparison would order lexicographically, so `9` would exceed `10`.
///
/// Temporal operands cast to nothing: PostgreSQL orders canonical RFC 3339 UTC
/// text identically to the instants it denotes, because a fixed-width,
/// fixed-offset representation sorts the same way lexicographically and
/// chronologically.
pub fn comparison_operand(scalar: &Scalar) -> (&'static str, String) {
    let cast = match scalar {
        Scalar::Number(_) => "::numeric",
        Scalar::Bool(_) => "::boolean",
        Scalar::Str(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => "",
    };
    (cast, scalar.literal_text())
}

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
    fn a_number_carries_a_numeric_cast_so_it_is_not_ordered_as_text() {
        assert_eq!(comparison_operand(&Scalar::Number(9.into())), ("::numeric", "9".to_string()));
    }

    #[test]
    fn a_boolean_carries_a_boolean_cast() {
        assert_eq!(comparison_operand(&Scalar::Bool(true)), ("::boolean", "true".to_string()));
    }

    #[test]
    fn text_and_temporal_operands_need_no_cast() {
        assert_eq!(comparison_operand(&Scalar::Str("red".to_string())), ("", "red".to_string()));
        let instant = andromeda_ngsi_ld_query::filter::scalar::parse_date_time("2024-01-01T00:00:00Z").expect("valid date-time");
        assert_eq!(comparison_operand(&Scalar::DateTime(instant)), ("", "2024-01-01T00:00:00Z".to_string()));
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
