//! The three `numeric` values beyond the decimals: `NaN`, `Infinity` and
//! `-Infinity`.
//!
//! PostgreSQL orders them as one total order: `-Infinity` below every finite
//! value, `Infinity` above, and `NaN` above `Infinity` and equal to itself. A
//! comparison that must order them otherwise, or an aggregate whose result
//! must stay finite, reads them out with these.

use crate::{cast::cast, literal::text_literal, pg_type::PostgresType};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};
use std::cmp::Ordering;
use strum::{EnumIter, IntoEnumIterator, IntoStaticStr};

/// One `numeric` value beyond the decimals, by the spelling PostgreSQL reads
/// it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, IntoStaticStr)]
pub enum NumericSpecial {
    /// `NaN`.
    #[strum(serialize = "NaN")]
    NotANumber,
    /// `Infinity`.
    #[strum(serialize = "Infinity")]
    Infinity,
    /// `-Infinity`.
    #[strum(serialize = "-Infinity")]
    NegativeInfinity,
}

impl NumericSpecial {
    /// How PostgreSQL orders the value against every finite `numeric`.
    #[must_use]
    pub const fn order_against_finite(self) -> Ordering {
        match self {
            NumericSpecial::NotANumber | NumericSpecial::Infinity => Ordering::Greater,
            NumericSpecial::NegativeInfinity => Ordering::Less,
        }
    }

    /// The value, spelled inline.
    fn literal(self) -> SimpleExpr {
        cast(text_literal(self.into()), PostgresType::Numeric)
    }
}

/// Whether `number` is `value`; `NULL` when `number` is `NULL`.
#[must_use]
pub fn is_special(number: SimpleExpr, value: NumericSpecial) -> SimpleExpr {
    number.eq(value.literal())
}

/// Whether `number` is not `value`; `NULL` when `number` is `NULL`.
#[must_use]
pub fn is_not_special(number: SimpleExpr, value: NumericSpecial) -> SimpleExpr {
    number.ne(value.literal())
}

/// `number` when it is finite, `NULL` otherwise.
#[must_use]
pub fn finite_numeric(number: SimpleExpr) -> SimpleExpr {
    CaseStatement::new()
        .case(number.clone().is_not_in(NumericSpecial::iter().map(NumericSpecial::literal)), number)
        .into()
}

#[cfg(test)]
mod tests {
    use crate::numeric_special::{NumericSpecial, finite_numeric, is_not_special, is_special};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};
    use std::cmp::Ordering;

    fn rendered(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_special_value_is_named_by_equality_and_left_out_by_inequality() {
        pretty_assertions::assert_eq!(
            rendered(is_not_special(Expr::col(Alias::new("n")), NumericSpecial::NotANumber)),
            r#"SELECT "n" <> ('NaN'::numeric)"#
        );
        pretty_assertions::assert_eq!(
            rendered(is_special(Expr::col(Alias::new("n")), NumericSpecial::NegativeInfinity)),
            r#"SELECT "n" = ('-Infinity'::numeric)"#
        );
    }

    #[test]
    fn a_finite_number_passes_and_the_three_special_values_fall_out() {
        pretty_assertions::assert_eq!(
            rendered(finite_numeric(Expr::col(Alias::new("n")))),
            r#"SELECT (CASE WHEN ("n" NOT IN ('NaN'::numeric, 'Infinity'::numeric, '-Infinity'::numeric)) THEN "n" END)"#
        );
    }

    /// PostgreSQL's `numeric` sorts `NaN` above every other value and the
    /// infinities at the two ends.
    #[test]
    fn not_a_number_and_infinity_order_above_a_finite_number_and_negative_infinity_below() {
        assert_eq!(NumericSpecial::NotANumber.order_against_finite(), Ordering::Greater);
        assert_eq!(NumericSpecial::Infinity.order_against_finite(), Ordering::Greater);
        assert_eq!(NumericSpecial::NegativeInfinity.order_against_finite(), Ordering::Less);
    }
}
