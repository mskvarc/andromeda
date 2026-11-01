//! The three `numeric` values beyond the decimals: `NaN`, `Infinity` and
//! `-Infinity`.
//!
//! PostgreSQL orders them as one total order: `-Infinity` below every finite
//! value, `Infinity` above, and `NaN` above `Infinity` and equal to itself. A
//! comparison that must leave `NaN` unordered, or an aggregate whose result
//! must stay finite, reads them out with these.

use crate::{cast::cast, literal::text_literal, pg_type::PostgresType};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};

/// The spellings PostgreSQL reads as the three values.
const SPECIAL_VALUES: [&str; 3] = ["NaN", "Infinity", "-Infinity"];

/// The `numeric` value `value`, spelled inline.
fn special(value: &str) -> SimpleExpr {
    cast(text_literal(value), PostgresType::Numeric)
}

/// Whether `number` is not `NaN`; `NULL` when `number` is `NULL`.
#[must_use]
pub fn is_not_nan(number: SimpleExpr) -> SimpleExpr {
    number.ne(special("NaN"))
}

/// `number` when it is finite, `NULL` otherwise.
#[must_use]
pub fn finite_numeric(number: SimpleExpr) -> SimpleExpr {
    // clone: the test and the result read one operand
    CaseStatement::new().case(number.clone().is_not_in(SPECIAL_VALUES.map(special)), number).into()
}

#[cfg(test)]
mod tests {
    use crate::numeric_special::{finite_numeric, is_not_nan};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn rendered(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn not_a_number_is_excluded_by_inequality() {
        pretty_assertions::assert_eq!(rendered(is_not_nan(Expr::col(Alias::new("n")))), r#"SELECT "n" <> ('NaN'::numeric)"#);
    }

    #[test]
    fn a_finite_number_passes_and_the_three_special_values_fall_out() {
        pretty_assertions::assert_eq!(
            rendered(finite_numeric(Expr::col(Alias::new("n")))),
            r#"SELECT (CASE WHEN ("n" NOT IN ('NaN'::numeric, 'Infinity'::numeric, '-Infinity'::numeric)) THEN "n" END)"#
        );
    }
}
