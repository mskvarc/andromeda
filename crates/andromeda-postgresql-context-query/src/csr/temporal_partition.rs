//! The clause 5.10.2.4 temporal partition of the registrations.
//!
//! Registrations describe either latest information (no interval) or temporal
//! information (an `observationInterval` or `managementInterval`), never both
//! (Table 5.2.9-1). A discovery without a temporal query considers only the
//! former; one with a temporal query considers only registrations whose
//! relevant interval is present and meets the window. An interval is present
//! exactly when its start is: Table 5.2.11-1 makes `startAt` mandatory.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrTemporalFilter, temporal::window::TimeInterval};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use chrono::{DateTime, Utc};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// One stored interval: the columns of its `startAt` and its `endAt`.
#[derive(Clone, Copy)]
struct IntervalColumns {
    /// `startAt`; `NULL` when the registration declares no such interval.
    start: Csr,
    /// `endAt`; `NULL` when the interval is open or absent.
    end: Csr,
}

/// `observationInterval`.
const OBSERVATION: IntervalColumns = IntervalColumns {
    start: Csr::ObservationIntervalStart,
    end: Csr::ObservationIntervalEnd,
};

/// `managementInterval`.
const MANAGEMENT: IntervalColumns = IntervalColumns {
    start: Csr::ManagementIntervalStart,
    end: Csr::ManagementIntervalEnd,
};

/// The registrations a discovery considers on the time axis.
///
/// Without a temporal query only registrations declaring neither interval
/// qualify. With one, the interval the `timeproperty` selects (clause 5.2.9:
/// `observedAt` reads `observationInterval`; `createdAt`, `modifiedAt` and
/// `deletedAt` read `managementInterval`) must be present and overlap the
/// period the query describes, which is the reading
/// `TimeInterval::meets` applies to a registration already in memory. The two
/// have to agree: a Context Source Registration Subscription matches in memory
/// what a discovery selects in SQL, and a client that reaches a source through
/// one and not the other has been told two different things about one clause.
pub(crate) fn temporal_partition(temporal: Option<&CompiledCsrTemporalFilter>) -> Condition {
    let Some(temporal) = temporal else {
        return Condition::all()
            .add(SeaExpr::col(OBSERVATION.start).is_null())
            .add(SeaExpr::col(MANAGEMENT.start).is_null());
    };
    let interval = match temporal.timeproperty.unwrap_or(TimeProp::Observed) {
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => MANAGEMENT,
        TimeProp::Observed => OBSERVATION,
    };
    // An unbounded window narrows nothing beyond the interval's presence.
    // Discovery cannot build one: a temporal filter needs both `timerel` and
    // `timeAt`. The interval type admits it, so it is answered here.
    let (from, to) = match temporal.interval {
        TimeInterval::Unbounded => (None, None),
        TimeInterval::Before(instant) => (None, Some(instant)),
        TimeInterval::After(instant) => (Some(instant), None),
        TimeInterval::Between { from, to } => (Some(from), Some(to)),
    };
    Condition::all()
        .add(SeaExpr::col(interval.start).is_not_null())
        .add(overlaps_window(interval, from, to))
}

/// `start <= to AND (end IS NULL OR from <= end)`, each half dropped when the
/// period is unbounded on that side: the period the query describes and the
/// interval the registration declares share at least one instant. An interval
/// without an end is open-ended (Table 5.2.11-1), so only a closed one is
/// tested at its end.
fn overlaps_window(interval: IntervalColumns, from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> Condition {
    let mut overlap = Condition::all();
    if let Some(to) = to {
        overlap = overlap.add(SeaExpr::col(interval.start).lte(window_instant(to)));
    }
    if let Some(from) = from {
        overlap = overlap.add(
            Condition::any()
                .add(SeaExpr::col(interval.end).is_null())
                .add(SeaExpr::col(interval.end).gte(window_instant(from))),
        );
    }
    overlap
}

/// A window instant bound as RFC 3339 text and cast in the statement, so it
/// compares as `timestamptz` against an interval column.
fn window_instant(instant: DateTime<Utc>) -> SimpleExpr {
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
        assert!(
            sql.contains("\"observation_interval_start\" IS NULL AND \"management_interval_start\" IS NULL"),
            "{sql}"
        );
    }

    /// Clause 5.10.2.4: with a temporal query the relevant interval must be
    /// present; a registration that lacks it does not match.
    #[test]
    fn a_temporal_query_requires_the_relevant_interval_to_be_present() {
        let sql = render(Some(&filter(TimeInterval::After(dt("2020-06-01T00:00:00Z")), None)));
        assert!(sql.contains("\"observation_interval_start\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains("\"observation_interval_start\" IS NULL"), "{sql}");
    }

    /// Clause 5.2.9: `observedAt` reads `observationInterval`; `createdAt`,
    /// `modifiedAt` and `deletedAt` read `managementInterval`.
    #[test]
    fn the_timeproperty_selects_the_interval() {
        for (timeproperty, interval) in [
            (None, "observation_interval"),
            (Some(TimeProp::Observed), "observation_interval"),
            (Some(TimeProp::Created), "management_interval"),
            (Some(TimeProp::Modified), "management_interval"),
            (Some(TimeProp::Deleted), "management_interval"),
        ] {
            let sql = render(Some(&filter(TimeInterval::After(dt("2020-06-01T00:00:00Z")), timeproperty)));
            assert!(sql.contains(&format!("\"{interval}_start\" IS NOT NULL")), "{timeproperty:?}: {sql}");
            assert!(sql.contains(&format!("\"{interval}_end\" IS NULL OR")), "{timeproperty:?}: {sql}");
        }
    }

    /// Clause 5.10.2.4: `before` describes everything up to its instant, so
    /// only the interval's start is tested and an interval starting later is
    /// out of reach.
    #[test]
    fn before_tests_only_the_interval_start() {
        let sql = render(Some(&filter(TimeInterval::Before(dt("2020-06-01T00:00:00Z")), None)));
        assert!(
            sql.contains("\"observation_interval_start\" <= ('2020-06-01T00:00:00+00:00'::timestamptz)"),
            "{sql}"
        );
        assert!(!sql.contains("\"observation_interval_end\""), "{sql}");
    }

    /// Clause 5.10.2.4: `after` describes everything from its instant onwards,
    /// so only the interval's end is tested and an open-ended interval is
    /// admitted whatever the instant (Table 5.2.11-1).
    #[test]
    fn after_tests_only_the_interval_end_and_admits_an_open_one() {
        let sql = render(Some(&filter(TimeInterval::After(dt("2020-06-01T00:00:00Z")), None)));
        assert!(
            sql.contains("\"observation_interval_end\" IS NULL OR \"observation_interval_end\" >= ('2020-06-01T00:00:00+00:00'::timestamptz)"),
            "{sql}"
        );
        assert!(!sql.contains("\"observation_interval_start\" <="), "{sql}");
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
            sql.contains("\"observation_interval_start\" <= ('2020-09-01T00:00:00+00:00'::timestamptz)"),
            "{sql}"
        );
        assert!(
            sql.contains("\"observation_interval_end\" >= ('2020-06-01T00:00:00+00:00'::timestamptz)"),
            "{sql}"
        );
    }
}
