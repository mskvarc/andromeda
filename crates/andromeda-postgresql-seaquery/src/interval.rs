//! Interval construction sea-query has no native form for.
//!
//! `make_interval(secs => n)` builds an `interval` from a number of seconds
//! using PostgreSQL's named-argument call form, which sea-query cannot express:
//! its function calls are positional. The seconds argument is passed as a bound
//! expression, so a lease duration computed at runtime never reaches the
//! statement as interpolated text.

use sea_query::{Expr, SimpleExpr};

/// `make_interval(secs => seconds)` - an `interval` of `seconds` seconds.
///
/// Used to move an instant by a runtime-computed duration, as in
/// `NOW() + make_interval(secs => $1)`, without spelling the duration into the
/// SQL or binding a pre-built `interval` string.
pub fn make_interval_secs(seconds: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("make_interval(secs => $1)", [seconds])
}

#[cfg(test)]
mod tests {
    use crate::interval::make_interval_secs;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_interval_is_built_from_a_bound_seconds_argument() {
        let (sql, values) = Query::select()
            .expr(Expr::current_timestamp().add(make_interval_secs(Expr::val(30.0_f64).into())))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("make_interval(secs => $1)"), "{sql}");
        assert_eq!(values.0.len(), 1, "the seconds argument is bound, not interpolated: {sql}");
    }
}
