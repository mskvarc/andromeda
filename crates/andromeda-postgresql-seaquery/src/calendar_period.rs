//! The grid of an aggregation period with a year or month part.
//!
//! `time_bucket` keeps only the year and month of its origin for such a period
//! and refuses one that also carries days or time, so the grid is computed here.
//! Boundary k of a period P from an anchor runs from the anchor moved by k
//! periods, in UTC, and every boundary is computed from the anchor: a boundary
//! stepped from the one before would carry a month-end clamp forward (31 January,
//! 28 February, 28 March), and the session time zone would move a boundary
//! across a daylight-saving change.
//!
//! The day of month a boundary falls on follows the anchor. An anchor on the
//! last day of its month puts every boundary on the last day of its month
//! (29 February, 31 March, 30 April). Any other anchor keeps its day of month,
//! clamped to the month's last day (30 January, 28 February, 30 March).
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

/// The start of period `index` of the grid of `period` from `origin`.
///
/// `origin` is a `timestamptz`, `period` an `interval` and `index` an integer;
/// the result is a `timestamptz`. The end of period k is the start of period
/// k + 1. A period with no month part is the origin moved by `index` periods.
/// Otherwise the months are counted from the origin's month, and the day of
/// month follows the origin's day rule (see the module documentation). Days and
/// time beside the months are then added `index` times over.
#[must_use]
pub fn calendar_period_start(period: SimpleExpr, origin: SimpleExpr, index: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(
        concat!(
            "CASE WHEN (extract(year from ($3)) * 12 + extract(month from ($3)))::integer = 0",
            " THEN (($1) AT TIME ZONE 'UTC') + ($2) * ($3)",
            " ELSE (",
            "date_trunc('month', ($1) AT TIME ZONE 'UTC')",
            " + make_interval(months => ($2)::integer * (extract(year from ($3)) * 12 + extract(month from ($3)))::integer)",
            " + make_interval(days => (",
            "CASE WHEN extract(day from ((($1) AT TIME ZONE 'UTC') + interval '1 day')) = 1",
            " THEN extract(day from (date_trunc('month', ($1) AT TIME ZONE 'UTC')",
            " + make_interval(months => ($2)::integer * (extract(year from ($3)) * 12 + extract(month from ($3)))::integer)",
            " + interval '1 month' - interval '1 day'))",
            " ELSE least(extract(day from (($1) AT TIME ZONE 'UTC')), extract(day from (date_trunc('month', ($1) AT TIME ZONE 'UTC')",
            " + make_interval(months => ($2)::integer * (extract(year from ($3)) * 12 + extract(month from ($3)))::integer)",
            " + interval '1 month' - interval '1 day')))",
            " END - 1)::integer)",
            " + (($1) AT TIME ZONE 'UTC' - date_trunc('day', ($1) AT TIME ZONE 'UTC'))",
            " + ($2) * (($3) - make_interval(months => (extract(year from ($3)) * 12 + extract(month from ($3)))::integer))",
            ") END AT TIME ZONE 'UTC'",
        ),
        [origin, index, period],
    )
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

    /// Every boundary is computed from the origin and the index, never from a
    /// boundary before it, and the result is read back in UTC.
    #[test]
    fn a_period_starts_at_the_origin_moved_by_whole_periods_in_utc() {
        let sql = render(calendar_period_start(column("period"), column("origin"), column("k")));
        assert!(sql.starts_with(r#"SELECT CASE WHEN (extract(year from ("period")) * 12 + extract(month from ("period")))::integer = 0 THEN (("origin") AT TIME ZONE 'UTC') + ("k") * ("period") ELSE"#), "{sql}");
        assert!(sql.ends_with("END AT TIME ZONE 'UTC'"), "{sql}");
        assert!(
            sql.contains(r#"make_interval(months => ("k")::integer * (extract(year from ("period")) * 12 + extract(month from ("period")))::integer)"#),
            "{sql}"
        );
    }

    /// The month-end branch: the last day of the target month when the origin is
    /// one, else the origin's day clamped to the target month's last day.
    #[test]
    fn a_calendar_boundary_takes_the_month_end_of_a_month_end_origin_and_the_clamped_day_otherwise() {
        let sql = render(calendar_period_start(column("period"), column("origin"), column("k")));
        assert!(
            sql.contains(r#"CASE WHEN extract(day from ((("origin") AT TIME ZONE 'UTC') + interval '1 day')) = 1 THEN extract(day from"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ELSE least(extract(day from (("origin") AT TIME ZONE 'UTC')), extract(day from"#),
            "{sql}"
        );
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
        assert!(sql.starts_with(r#"SELECT ("estimate") - ("#), "{sql}");
        assert!(sql.contains(r#" > ("observed_at"))::integer"#), "{sql}");
        assert!(sql.ends_with(r#"<= ("observed_at"))::integer"#), "{sql}");
    }
}
