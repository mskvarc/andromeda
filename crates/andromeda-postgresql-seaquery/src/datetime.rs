//! The PostgreSQL functions a stored temporal text is validated and rendered through.
//!
//! None has a native sea-query constructor: `to_char` formats a resolved
//! instant back to fixed-width text, `pg_input_is_valid` (PostgreSQL 16+)
//! asks a type's input function whether a candidate text is a real value
//! without raising, `translate` writes a decimal comma as the point that
//! input function reads, and `regexp_replace` writes the ISO 8601 year `0000`
//! as the year that input function names it by. Each is spelled here as
//! [`Func::cust`] so the function name is written once and no value is
//! interpolated into the call.

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

/// The `translate(text, from, to)` character substitution function.
#[derive(Iden, Clone, Copy)]
#[iden = "translate"]
struct Translate;

/// The `regexp_replace(text, pattern, replacement)` substitution function.
#[derive(Iden, Clone, Copy)]
#[iden = "regexp_replace"]
struct RegexpReplace;

/// A text opening with the ISO 8601 year `0000`, the rest captured.
const YEAR_ZERO: &str = "^0000(.*)$";

/// The year `0000` as PostgreSQL's input functions name it: the year 1 before
/// the era, marked by a trailing `BC`.
const YEAR_ONE_BEFORE_THE_ERA: &str = "0001\\1 BC";

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

/// `translate(text, ',', '.')` - a time text with its decimal comma written as
/// the decimal point.
///
/// Clause 4.6.3 admits either before the fractional seconds, and the
/// `timestamptz` input function reads only the point.
#[must_use]
pub fn with_decimal_point(text: SimpleExpr) -> SimpleExpr {
    Func::cust(Translate).args([text, text_literal(","), text_literal(".")]).into()
}

/// `regexp_replace(text, '^0000(.*)$', '0001\1 BC')` - a date or time text
/// with the ISO 8601 year `0000` written as the `timestamptz` and `date` input
/// functions read it, any other text as it is.
///
/// ISO 8601, which clause 4.6.3 defines its `DateTime` by, counts the year
/// before year 1 as `0000`, the proleptic Gregorian year 1 BC; PostgreSQL's
/// input functions refuse that year and read it as `0001` followed by `BC`,
/// the same day and the same leap year.
#[must_use]
pub fn with_iso_year_zero(text: SimpleExpr) -> SimpleExpr {
    Func::cust(RegexpReplace)
        .args([text, text_literal(YEAR_ZERO), text_literal(YEAR_ONE_BEFORE_THE_ERA)])
        .into()
}

#[cfg(test)]
mod tests {
    use crate::{
        datetime::{pg_input_is_valid, to_char, with_decimal_point, with_iso_year_zero},
        pg_type::PostgresType,
    };
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn to_char_renders_the_function_with_its_inline_format() {
        let sql = render(to_char(Expr::col(Alias::new("observed_at")), "FM9999"));
        assert!(sql.contains("to_char("), "{sql}");
        assert!(sql.contains("'FM9999'"), "the format is inlined, not bound: {sql}");
    }

    #[test]
    fn a_decimal_comma_is_translated_to_the_point() {
        let sql = render(with_decimal_point(Expr::col(Alias::new("written"))));
        assert!(sql.contains(r#"translate("written", ',', '.')"#), "{sql}");
    }

    #[test]
    fn the_iso_year_zero_is_written_as_the_year_one_before_the_era() {
        let sql = render(with_iso_year_zero(Expr::col(Alias::new("written"))));
        assert!(sql.contains("regexp_replace(\"written\", '^0000(.*)$', "), "{sql}");
        assert!(sql.contains(r"E'0001\\1 BC'"), "{sql}");
    }

    #[test]
    fn pg_input_is_valid_names_the_type_it_validates_against() {
        let sql = render(pg_input_is_valid(Expr::col(Alias::new("stored")), PostgresType::Timestamptz));
        assert!(sql.contains("pg_input_is_valid("), "{sql}");
        assert!(sql.contains("'timestamptz'"), "{sql}");
    }
}
