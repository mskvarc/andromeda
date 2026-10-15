//! The open ends of an instant range, standing in for a bound that is absent.
//!
//! A range whose bound is NULL has no end on that side. Coalescing the bound
//! to `infinity` or `-infinity` keeps the comparison a single range condition
//! an index can serve, where `bound IS NULL OR value <= bound` would leave it
//! a filter over every row. Both sentinels are rendered as typed literals
//! rather than bound as parameters: an expression index matches a literal
//! structurally, and a bound value matches only under a custom plan.

use sea_query::{Expr, Func, SimpleExpr};

/// The typed literal above every real instant.
const END_OF_TIME: &str = "'infinity'::timestamptz";

/// The typed literal below every real instant.
const START_OF_TIME: &str = "'-infinity'::timestamptz";

/// PostgreSQL: `COALESCE(instant, 'infinity'::timestamptz)`, an upper bound
/// that is absent when `instant` is NULL.
#[must_use]
pub fn or_end_of_time(instant: SimpleExpr) -> SimpleExpr {
    Func::coalesce([instant, Expr::cust(END_OF_TIME)]).into()
}

/// PostgreSQL: `COALESCE(instant, '-infinity'::timestamptz)`, a lower bound
/// that is absent when `instant` is NULL.
#[must_use]
pub fn or_start_of_time(instant: SimpleExpr) -> SimpleExpr {
    Func::coalesce([instant, Expr::cust(START_OF_TIME)]).into()
}

#[cfg(test)]
mod tests {
    use crate::open_instant::{or_end_of_time, or_start_of_time};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_absent_upper_bound_is_the_end_of_time() {
        let sql = render(or_end_of_time(Expr::col(Alias::new("edge"))));
        assert!(sql.contains(r#"COALESCE("edge", 'infinity'::timestamptz)"#), "{sql}");
    }

    #[test]
    fn an_absent_lower_bound_is_the_start_of_time() {
        let sql = render(or_start_of_time(Expr::col(Alias::new("edge"))));
        assert!(sql.contains(r#"COALESCE("edge", '-infinity'::timestamptz)"#), "{sql}");
    }
}
