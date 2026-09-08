//! The PostgreSQL datetime functions a stored temporal text is validated and
//! rendered through.
//!
//! Neither has a native sea-query constructor: `to_char` formats a resolved
//! instant back to fixed-width text, and `pg_input_is_valid` (PostgreSQL 16+)
//! asks a type's input function whether a candidate text is a real value without
//! raising. Both are spelled here as [`Func::cust`] so the function name is
//! written once and no value is interpolated into the call.

use crate::{literal::text_literal, pg_type::PostgresType};
use sea_query::{Func, Iden, SimpleExpr};

/// The `to_char(value, format)` formatting function.
#[derive(Iden, Clone, Copy)]
#[iden = "to_char"]
struct ToChar;

/// The `pg_input_is_valid(text, type)` validation function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_input_is_valid"]
struct PgInputIsValid;

/// `to_char(value, 'format')` - render `value` as text under a `format` template.
///
/// The template is a fixed statement-level constant, not caller-supplied data, so
/// it is inlined as a literal rather than bound.
#[must_use]
pub fn to_char(value: SimpleExpr, format: &str) -> SimpleExpr {
    Func::cust(ToChar).args([value, text_literal(format)]).into()
}

/// `pg_input_is_valid(value, 'type')` - whether `value` is valid input for `ty`,
/// answered by the type's own input function without raising.
///
/// The type name is a statement-level constant naming the type token, so it is
/// inlined as a literal exactly as a [`crate::cast`] names its target type.
#[must_use]
pub fn pg_input_is_valid(value: SimpleExpr, ty: PostgresType) -> SimpleExpr {
    let type_name = Iden::to_string(&ty);
    Func::cust(PgInputIsValid).args([value, text_literal(&type_name)]).into()
}

#[cfg(test)]
mod tests {
    use crate::{
        datetime::{pg_input_is_valid, to_char},
        pg_type::PostgresType,
    };
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn to_char_renders_the_function_with_its_inline_format() {
        let sql = render(to_char(Expr::col(Alias::new("observed_at")), "HH24:MI:SS.US"));
        assert!(sql.contains("to_char("), "{sql}");
        assert!(sql.contains("'HH24:MI:SS.US'"), "the format is inlined, not bound: {sql}");
    }

    #[test]
    fn pg_input_is_valid_names_the_type_it_validates_against() {
        let sql = render(pg_input_is_valid(Expr::col(Alias::new("stored")), PostgresType::Timestamptz));
        assert!(sql.contains("pg_input_is_valid("), "{sql}");
        assert!(sql.contains("'timestamptz'"), "{sql}");
    }
}
