//! Interval construction and interval grids sea-query has no native form for.
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
#[must_use]
pub fn make_interval_secs(seconds: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("make_interval(secs => $1)", [seconds])
}

/// `EXTRACT(EPOCH FROM interval)` - the length of an `interval` in seconds.
#[must_use]
pub fn epoch_seconds(interval: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("EXTRACT(EPOCH FROM $1)", [interval])
}

/// `count * interval '1 microsecond'` - an `interval` of `count` microseconds,
/// exact for an integer count where a fractional-seconds interval rounds.
#[must_use]
pub fn microseconds(count: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 * interval '1 microsecond'", [count])
}

/// The first point strictly after `after` of the grid `origin + k * period`
/// (`k >= 1`), for an `after` at or past `origin`: `origin + make_interval(secs
/// => period * (floor(extract(epoch from (after - origin)) / period) + 1))`.
///
/// `period` is in seconds and may be fractional. `extract(epoch from ...)`
/// reads the elapsed interval as seconds, so the grid needs no calendar
/// arithmetic.
#[must_use]
pub fn next_grid_point(origin: SimpleExpr, period: SimpleExpr, after: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(
        "$1 + make_interval(secs => $2 * (floor(extract(epoch from ($3 - $4)) / $5) + 1))",
        [origin.clone(), period.clone(), after, origin, period],
    )
}

#[cfg(test)]
mod tests {
    use crate::interval::{epoch_seconds, make_interval_secs, microseconds, next_grid_point};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn the_interval_is_built_from_a_bound_seconds_argument() {
        let (sql, values) = Query::select()
            .expr(Expr::current_timestamp().add(make_interval_secs(Expr::val(30.0_f64))))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("make_interval(secs => $1)"), "{sql}");
        assert_eq!(values.0.len(), 1, "the seconds argument is bound, not interpolated: {sql}");
    }

    #[test]
    fn the_epoch_of_an_interval_is_its_length_in_seconds() {
        let sql = Query::select()
            .expr(epoch_seconds(Expr::col(Alias::new("time_interval"))))
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT EXTRACT(EPOCH FROM "time_interval")"#);
    }

    #[test]
    fn microseconds_scale_the_unit_interval_by_a_bound_count() {
        let (sql, values) = Query::select().expr(microseconds(Expr::val(7_i64))).build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT $1 * interval '1 microsecond'");
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn the_next_grid_point_counts_the_whole_periods_elapsed_since_the_origin() {
        let sql = Query::select()
            .expr(next_grid_point(
                Expr::col(Alias::new("created_at")),
                Expr::col(Alias::new("period")),
                Expr::col(Alias::new("answered_at")),
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "created_at" + make_interval(secs => "period" * (floor(extract(epoch from ("answered_at" - "created_at")) / "period") + 1))"#
        );
    }
}
