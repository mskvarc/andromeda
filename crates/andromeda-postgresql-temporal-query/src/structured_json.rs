//! The JSON value the structured column of one `attribute_history` row stands
//! for.
//!
//! The structured column keeps a Property's one structured value in its
//! expanded form, so a JSON literal -- a `value` written as `{"@type": "@json",
//! "@value": ...}` (clause 4.5.2) -- is stored as that value object, with the
//! JSON the client wrote under `@value`. Tables 4.5.19.1-1 classify and measure
//! the value the client wrote: a JSON Array has `avg`, `min`, `max` and `sum`
//! over its size, a JSON Object has none of them. Every expression that
//! classifies or measures the column therefore reads the literal out of its
//! value object first; any other stored JSON already is the value.

use crate::alias::Ah;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{literal::text_literal, pg_operator::PgBinaryOperator};
use sea_query::{CaseStatement, Expr, ExprTrait, Func, SimpleExpr};

/// The JSON-LD keyword naming a value object's datatype.
const TYPE_KEYWORD: &str = "@type";

/// The JSON-LD keyword holding a value object's value.
const VALUE_KEYWORD: &str = "@value";

/// The datatype of a JSON literal value object.
const JSON_DATATYPE: &str = "@json";

/// The JSON the structured column of the row aliased `ah` stands for: the
/// literal inside a `@json` value object, and the stored JSON otherwise.
///
/// `->>` answers SQL NULL for an array or for an object without `@type`, and a
/// node object's `@type` is an IRI or an array of them, never `@json`, so only
/// a JSON literal takes the first branch.
///
/// The literal is taken with `COALESCE` rather than an `ELSE` branch: `->`
/// yields the JSONB `null` for a literal written as `null`, never SQL NULL, so
/// the stored JSON is the fallback only for a row that holds no literal.
#[must_use]
pub fn structured_json() -> SimpleExpr {
    let value_object = || SimpleExpr::from(Expr::col((Ah, AttributeHistory::ValueObject)));
    let literal: SimpleExpr = CaseStatement::new()
        .case(
            value_object().json_get_text(text_literal(TYPE_KEYWORD)).eq(text_literal(JSON_DATATYPE)),
            value_object().json_get(text_literal(VALUE_KEYWORD)),
        )
        .into();
    Func::coalesce([literal, value_object()]).into()
}

#[cfg(test)]
mod tests {
    use crate::structured_json::structured_json;
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered() -> (String, usize) {
        let (sql, values) = Query::select().expr(structured_json()).to_owned().build(PostgresQueryBuilder);
        (sql, values.0.len())
    }

    /// A JSON literal is read out of its `@json` value object.
    #[test]
    fn a_json_literal_is_read_out_of_its_value_object() {
        let (sql, _) = rendered();
        assert!(sql.contains(r#"WHEN (("ah"."value_object" ->> ('@type')) = ('@json'))"#), "{sql}");
        assert!(sql.contains(r#"THEN "ah"."value_object" -> ('@value') END"#), "{sql}");
    }

    /// Every other stored JSON is the value itself.
    #[test]
    fn any_other_stored_json_is_the_value_itself() {
        let (sql, _) = rendered();
        assert!(sql.contains("COALESCE((CASE WHEN"), "{sql}");
        assert!(sql.ends_with(r#"END), "ah"."value_object")"#), "{sql}");
    }

    /// The keywords are rendered inline, so the expression shifts no binding a
    /// caller made.
    #[test]
    fn the_expression_binds_no_parameter() {
        pretty_assertions::assert_eq!(rendered().1, 0);
    }
}
