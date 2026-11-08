//! The grid of an aggregation period with a year or month part.
//!
//! `time_bucket` keeps only the year and month of its origin for such a period
//! and refuses one that also carries days or time, so the grid is computed here:
//! period k of a period P from an origin runs from `origin + k * P` to
//! `origin + (k + 1) * P`. Every boundary is computed from the origin, in UTC: a
//! boundary stepped from the one before would carry a month-end clamp forward
//! (31 January, 28 February, 28 March), and the session time zone would move a
//! boundary across a daylight-saving change.
//!
//! The index of an instant's period is first estimated from the elapsed seconds
//! over the period's nominal length, a month counted as the Gregorian average of
//! 30.436875 days and a year as 365.2425 days, then corrected by one either way.
//! Over its 400-year cycle the calendar averages exactly those lengths, so a
//! boundary departs from its nominal position by under a week (the running
//! difference of month lengths plus the day clamp), while a period with a month
//! part lasts at least 28 days: one correction always suffices. The nominal
//! length is read from the interval's own fields, because PostgreSQL folds a
//! fractional month or year into days (`P0.5M` is 15 days), and an estimate from
//! any other length would drift until it passed a whole period.
//!
//! Each function takes its period and origin as expressions the caller projects
//! once (a column of an enclosing subquery), since every one of them reads them
//! several times.

use sea_query::{Expr, SimpleExpr};

/// The start of period `index` of the grid of `period` from `origin`:
/// `((origin AT TIME ZONE 'UTC') + index * period) AT TIME ZONE 'UTC'`.
///
/// `origin` is a `timestamptz`, `period` an `interval` and `index` an integer;
/// the result is a `timestamptz`. The end of period k is the start of period
/// k + 1.
#[must_use]
pub fn calendar_period_start(period: SimpleExpr, origin: SimpleExpr, index: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("((($1) AT TIME ZONE 'UTC') + ($2) * ($3)) AT TIME ZONE 'UTC'", [origin, index, period])
}

/// The estimated index of the period `instant` falls in: the whole nominal
/// periods elapsed from `origin`, a `bigint` at most one away from the index.
#[must_use]
pub fn calendar_period_estimate(period: SimpleExpr, origin: SimpleExpr, instant: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(
        "floor(extract(epoch from ((($1) AT TIME ZONE 'UTC') - (($2) AT TIME ZONE 'UTC'))) /((extract(year from $3) * 12 + extract(month from $4)) * 2629746 + extract(day from $5) * 86400 + extract(hour from $6) * 3600 + extract(minute from $7) * 60 + extract(second from $8)))::bigint",
        [
            instant,
            origin,
            period.clone(),
            period.clone(),
            period.clone(),
            period.clone(),
            period.clone(),
            period,
        ],
    )
}

/// The index of the period `instant` falls in, from its `estimate`: one less
/// when the estimated period starts after the instant, one more when the next
/// one starts at or before it.
#[must_use]
pub fn calendar_period_index(period: SimpleExpr, origin: SimpleExpr, instant: SimpleExpr, estimate: SimpleExpr) -> SimpleExpr {
    let estimated_start = calendar_period_start(period.clone(), origin.clone(), estimate.clone());
    let next_start = calendar_period_start(period, origin, Expr::cust_with_exprs("$1 + 1", [estimate.clone()]));
    Expr::cust_with_exprs(
        "($1) - (($2) > ($3))::integer + (($4) <= ($5))::integer",
        [estimate, estimated_start, instant.clone(), next_start, instant],
    )
}

#[cfg(test)]
mod tests {
    use crate::calendar_period::{calendar_period_estimate, calendar_period_index, calendar_period_start};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn column(name: &str) -> SimpleExpr {
        Expr::col(Alias::new(name))
    }

    #[test]
    fn a_period_starts_at_the_origin_moved_by_whole_periods_in_utc() {
        let sql = render(calendar_period_start(column("period"), column("origin"), column("k")));
        assert_eq!(sql, r#"SELECT ((("origin") AT TIME ZONE 'UTC') + ("k") * ("period")) AT TIME ZONE 'UTC'"#);
    }

    /// The nominal month is 2,629,746 s (30.436875 days) and the nominal year
    /// twelve of them, the Gregorian averages; days and time are exact.
    #[test]
    fn the_estimate_divides_the_elapsed_utc_seconds_by_the_nominal_length_of_the_interval_fields() {
        let sql = render(calendar_period_estimate(column("period"), column("origin"), column("observed_at")));
        assert!(
            sql.contains(r#"extract(epoch from ((("observed_at") AT TIME ZONE 'UTC') - (("origin") AT TIME ZONE 'UTC')))"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(extract(year from "period") * 12 + extract(month from "period")) * 2629746"#),
            "{sql}"
        );
        assert!(sql.contains(r#"extract(day from "period") * 86400"#), "{sql}");
        assert!(sql.contains(r#"extract(second from "period"))"#), "{sql}");
        assert!(sql.starts_with("SELECT floor(") && sql.ends_with(")::bigint"), "{sql}");
    }

    #[test]
    fn the_index_corrects_the_estimate_by_the_two_boundaries_around_it() {
        let sql = render(calendar_period_index(
            column("period"),
            column("origin"),
            column("observed_at"),
            column("estimate"),
        ));
        assert_eq!(
            sql,
            r#"SELECT ("estimate") - ((((("origin") AT TIME ZONE 'UTC') + ("estimate") * ("period")) AT TIME ZONE 'UTC') > ("observed_at"))::integer + ((((("origin") AT TIME ZONE 'UTC') + ("estimate" + 1) * ("period")) AT TIME ZONE 'UTC') <= ("observed_at"))::integer"#
        );
    }
}
