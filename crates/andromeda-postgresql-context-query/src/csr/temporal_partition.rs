//! The clause 5.10.2.4 temporal partition of the registrations.
//!
//! Registrations describe either latest information (no interval) or temporal
//! information (an `observationInterval` or `managementInterval`), never both
//! (Table 5.2.9-1). A discovery without a temporal query considers only the
//! former; one with a temporal query considers only registrations whose
//! relevant interval is present and meets the window.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrTemporalFilter, temporal::window::TimeInterval};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use chrono::{DateTime, Utc};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// The registrations a discovery considers on the time axis.
///
/// Without a temporal query only registrations declaring neither interval
/// qualify. With one, the interval the `timeproperty` selects (clause 5.2.9:
/// `observedAt` reads `observationInterval`; `createdAt`, `modifiedAt` and
/// `deletedAt` read `managementInterval`) must be present and meet the window:
/// for `before` and `after` the instant lies within the interval or on one of
/// its endpoints, for `between` the two intervals overlap.
pub(crate) fn temporal_partition(temporal: Option<&CompiledCsrTemporalFilter>) -> Condition {
    let Some(temporal) = temporal else {
        return Condition::all()
            .add(SeaExpr::col(Csr::ObservationInterval).is_null())
            .add(SeaExpr::col(Csr::ManagementInterval).is_null());
    };
    let interval_col = match temporal.timeproperty.unwrap_or(TimeProp::Observed) {
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => Csr::ManagementInterval,
        TimeProp::Observed => Csr::ObservationInterval,
    };
    let present = Condition::all().add(SeaExpr::col(interval_col).is_not_null());
    match temporal.interval {
        // An unbounded window narrows nothing beyond the interval's presence.
        // Discovery cannot build one — a temporal filter needs both `timerel`
        // and `timeAt` — but the interval type admits it.
        TimeInterval::Unbounded => present,
        TimeInterval::Before(instant) | TimeInterval::After(instant) => present.add(contains_instant(interval_col, instant)),
        TimeInterval::Between { from, to } => present.add(overlaps_window(interval_col, from, to)),
    }
}

/// `startAt <= instant AND (endAt IS NULL OR instant <= endAt)`: the instant
/// is inside the interval or on one of its endpoints.
fn contains_instant(interval_col: Csr, instant: DateTime<Utc>) -> Condition {
    Condition::all()
        .add(SeaExpr::expr(interval_bound(interval_col, "startAt")).lte(interval_instant(instant)))
        .add(open_or(
            interval_col,
            SeaExpr::expr(interval_bound(interval_col, "endAt")).gte(interval_instant(instant)),
        ))
}

/// `startAt <= to AND (endAt IS NULL OR from <= endAt)`: the window and the
/// interval share at least one instant.
fn overlaps_window(interval_col: Csr, from: DateTime<Utc>, to: DateTime<Utc>) -> Condition {
    Condition::all()
        .add(SeaExpr::expr(interval_bound(interval_col, "startAt")).lte(interval_instant(to)))
        .add(open_or(
            interval_col,
            SeaExpr::expr(interval_bound(interval_col, "endAt")).gte(interval_instant(from)),
        ))
}

/// `endAt IS NULL OR <bounded>`: an interval without an end is open-ended
/// (Table 5.2.11-1), so only a closed one is tested at its end.
fn open_or(interval_col: Csr, bounded: SimpleExpr) -> Condition {
    Condition::any()
        .add(SeaExpr::expr(interval_bound(interval_col, "endAt")).is_null())
        .add(bounded)
}

/// One bound of a registration's stored interval, lifted to `timestamptz`:
/// `("interval_col"->>'member')::timestamptz`.
fn interval_bound(interval_col: Csr, member: &str) -> SimpleExpr {
    cast_compound(
        SimpleExpr::from(SeaExpr::col(interval_col)).json_get_text(text_literal(member)),
        PostgresType::Timestamptz,
    )
}

/// A window instant bound as RFC 3339 text and cast in the statement, so it
/// compares as `timestamptz` against a lifted interval bound.
fn interval_instant(instant: DateTime<Utc>) -> SimpleExpr {
    cast(SeaExpr::val(instant.to_rfc3339()), PostgresType::Timestamptz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn dt(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).expect("valid instant").with_timezone(&Utc)
    }

    fn render(temporal: Option<&CompiledCsrTemporalFilter>) -> String {
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .cond_where(temporal_partition(temporal))
            .to_string(PostgresQueryBuilder)
    }

    fn filter(interval: TimeInterval, timeproperty: Option<TimeProp>) -> CompiledCsrTemporalFilter {
        CompiledCsrTemporalFilter::builder().interval(interval).timeproperty(timeproperty).build()
    }

    /// Clause 5.10.2.4: without a temporal query only latest-information
    /// registrations, those declaring no interval at all, are considered.
    #[test]
    fn no_temporal_query_admits_only_registrations_without_any_interval() {
        let sql = render(None);
        assert!(sql.contains("\"observation_interval\" IS NULL AND \"management_interval\" IS NULL"), "{sql}");
    }

    /// Clause 5.10.2.4: with a temporal query the relevant interval must be
    /// present; a registration that lacks it does not match.
    #[test]
    fn a_temporal_query_requires_the_relevant_interval_to_be_present() {
        let sql = render(Some(&filter(TimeInterval::After(dt("2020-06-01T00:00:00Z")), None)));
        assert!(sql.contains("\"observation_interval\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains("\"observation_interval\" IS NULL OR"), "{sql}");
    }

    /// Clause 5.2.9: `observedAt` reads `observationInterval`; `createdAt`,
    /// `modifiedAt` and `deletedAt` read `managementInterval`.
    #[test]
    fn the_timeproperty_selects_the_interval() {
        for (timeproperty, column) in [
            (None, "observation_interval"),
            (Some(TimeProp::Observed), "observation_interval"),
            (Some(TimeProp::Created), "management_interval"),
            (Some(TimeProp::Modified), "management_interval"),
            (Some(TimeProp::Deleted), "management_interval"),
        ] {
            let sql = render(Some(&filter(TimeInterval::After(dt("2020-06-01T00:00:00Z")), timeproperty)));
            assert!(sql.contains(&format!("\"{column}\" IS NOT NULL")), "{timeproperty:?}: {sql}");
        }
    }

    /// Clause 5.10.2.4: for `before` and `after` the instant must lie within
    /// the interval or on an endpoint, and an open interval has no upper test.
    #[test]
    fn before_and_after_test_the_instant_against_both_bounds_with_an_open_end_admitted() {
        for interval in [
            TimeInterval::Before(dt("2020-06-01T00:00:00Z")),
            TimeInterval::After(dt("2020-06-01T00:00:00Z")),
        ] {
            let sql = render(Some(&filter(interval, None)));
            assert!(
                sql.contains("->> ('startAt'))::timestamptz) <= ('2020-06-01T00:00:00+00:00'::timestamptz)"),
                "{sql}"
            );
            assert!(
                sql.contains(
                    "->> ('endAt'))::timestamptz) IS NULL OR ((\"observation_interval\" ->> ('endAt'))::timestamptz) >= ('2020-06-01T00:00:00+00:00'::timestamptz)"
                ),
                "{sql}"
            );
        }
    }

    /// Clause 5.10.2.4: `between` matches when the two intervals overlap.
    #[test]
    fn between_tests_for_overlap() {
        let sql = render(Some(&filter(
            TimeInterval::Between {
                from: dt("2020-06-01T00:00:00Z"),
                to: dt("2020-09-01T00:00:00Z"),
            },
            None,
        )));
        assert!(
            sql.contains("->> ('startAt'))::timestamptz) <= ('2020-09-01T00:00:00+00:00'::timestamptz)"),
            "{sql}"
        );
        assert!(
            sql.contains("->> ('endAt'))::timestamptz) >= ('2020-06-01T00:00:00+00:00'::timestamptz)"),
            "{sql}"
        );
    }
}
