//! The JSON type of a JSONB value, and the readings of a value guarded by it.
//!
//! A cast out of JSONB raises on a value of another JSON type, and only a
//! `CASE` guarantees its test runs before its result, so a reading that keeps a
//! value of one JSON type tests the type in a `CASE` and yields `NULL` for any
//! other.

use crate::{cast::cast_compound, jsonb::jsonb_typeof, literal::text_literal, pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};
use strum::IntoStaticStr;

/// A JSON type, spelled as `jsonb_typeof` names it.
#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum JsonType {
    /// A JSON object.
    Object,
    /// A JSON array.
    Array,
    /// A JSON string.
    String,
    /// A JSON number.
    Number,
    /// A JSON boolean.
    Boolean,
    /// The JSON literal `null`.
    Null,
}

/// Whether `value` is of `json_type`: `jsonb_typeof(value) = '<type>'`.
#[must_use]
pub fn jsonb_type_is(value: SimpleExpr, json_type: JsonType) -> SimpleExpr {
    jsonb_typeof(value).eq(text_literal(json_type.into()))
}

/// The whole text of a JSONB value: `(value) #>> '{}'`.
///
/// The empty `text[]` path yields the value itself, a string without its
/// quotes and any other value in its JSON spelling.
#[must_use]
pub fn jsonb_whole_text(value: SimpleExpr) -> SimpleExpr {
    value.json_path_text(text_literal("{}"))
}

/// `value` read as `ty` when its JSON type is `json_type`, `NULL` otherwise.
#[must_use]
pub fn jsonb_typed_reading(value: SimpleExpr, json_type: JsonType, ty: PostgresType) -> SimpleExpr {
    CaseStatement::new()
        .case(jsonb_type_is(value.clone(), json_type), cast_compound(jsonb_whole_text(value), ty)) // clone: the test reads the value it guards
        .into()
}

#[cfg(test)]
mod tests {
    use crate::{
        json_type::{JsonType, jsonb_type_is, jsonb_typed_reading, jsonb_whole_text},
        pg_type::PostgresType,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    /// Render one expression as the whole text of a `SELECT`.
    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn every_json_type_is_spelled_as_jsonb_typeof_names_it() {
        let names: [&str; 6] = [
            JsonType::Object.into(),
            JsonType::Array.into(),
            JsonType::String.into(),
            JsonType::Number.into(),
            JsonType::Boolean.into(),
            JsonType::Null.into(),
        ];
        assert_eq!(names, ["object", "array", "string", "number", "boolean", "null"]);
    }

    #[test]
    fn a_type_test_compares_the_jsonb_typeof_name() {
        let sql = render(jsonb_type_is(Expr::col(Alias::new("v")), JsonType::Array));
        assert_eq!(sql, r#"SELECT jsonb_typeof("v") = ('array')"#);
    }

    #[test]
    fn the_whole_text_reads_the_empty_path() {
        let sql = render(jsonb_whole_text(Expr::col(Alias::new("v"))));
        assert_eq!(sql, r#"SELECT "v" #>> ('{}')"#);
    }

    #[test]
    fn a_typed_reading_casts_only_a_value_of_its_json_type() {
        let sql = render(jsonb_typed_reading(Expr::col(Alias::new("v")), JsonType::Number, PostgresType::Numeric));
        assert_eq!(sql, r#"SELECT (CASE WHEN (jsonb_typeof("v") = ('number')) THEN ("v" #>> ('{}'))::numeric END)"#);
    }
}
