//! The clause 4.18 validity interval of one stored scope event.
//!
//! Clause 4.18: "For the Temporal Evolution a given Scope is considered valid
//! from the time it has been set until the time it has been explicitly removed
//! by an update or delete operation (for an example see annex C, clause
//! C.5.16)." A `scope_history` row is therefore not a point on the time axis
//! but the left end of an interval that runs until the entity's next scope
//! event, and every temporal reading of scope history — the `scopeQ` of clause
//! 4.19 and the reified scope of clause 4.5.6 alike — asks about that interval
//! rather than about the instant.
//!
//! The right end is never stored. Clause 4.18 gives an Entity at most one
//! instance of the scope Property, so every write replaces the whole value and
//! any later event closes the one before it; the interval is reconstructed from
//! that rather than from a column, which is also why a removal need not name
//! the scope it ended — and why `chk_scope_history_value_presence` can forbid
//! it from naming one.

use crate::{
    history_source::{HistoryBranch, HistoryOf, scope_rows::scope_rows},
    window_reach::WindowReach,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_postgresql_schema::temporal::ScopeHistory;
use sea_query::{Condition, Expr, ExprTrait, Iden, Query, SimpleExpr};

/// The `scope_history` alias one scope event is read under.
///
/// Both readings correlate a nested supersession probe to the row they stand
/// on, so the outer alias is part of the shared vocabulary rather than each
/// statement's own choice.
#[derive(Iden, Clone, Copy)]
#[iden = "sh"]
pub struct ScopeEvent;

/// The `scope_history` alias the supersession probe reads under, which names
/// the events that could have ended the one [`ScopeEvent`] stands on.
#[derive(Iden, Clone, Copy)]
#[iden = "sh_next"]
struct SupersedingScopeEvent;

/// The `scope_history` column a `timeproperty` positions a scope event on.
///
/// Clause 5.2.21 admits the four Temporal Properties and clause 4.5.6 gives the
/// reified scope every one of them, so each axis answers from its own column.
/// `deletedAt` is carried by removals alone, which makes it the degenerate
/// axis: a removal names no scope path, so no scope pattern can be matched on
/// it and no scope is reported as valid over it.
#[must_use]
pub const fn scope_history_axis(timeproperty: TimeProp) -> ScopeHistory {
    match timeproperty {
        TimeProp::Observed => ScopeHistory::ObservedAt,
        TimeProp::Created => ScopeHistory::CreatedAt,
        TimeProp::Modified => ScopeHistory::ModifiedAt,
        TimeProp::Deleted => ScopeHistory::DeletedAt,
    }
}

/// Whether the scope event is positioned on `axis` at all.
///
/// An event carrying no instant on the requested axis has no place in the
/// reconstruction: it neither opens an interval nor closes one, rather than
/// sorting as though it were the earliest event the entity has.
#[must_use]
pub fn positioned_on_axis(axis: ScopeHistory) -> SimpleExpr {
    Expr::col((ScopeEvent, axis)).is_not_null()
}

/// Whether the validity interval the scope event opens overlaps `interval`.
///
/// The event at `T` is valid over `[T, next)`, so it overlaps a window
/// `[from, to)` exactly when `T < to` and nothing superseded it at or before
/// `from`. An open end of the window drops the predicate that would have
/// bounded it: a window with no closing instant admits an event of any age, and
/// a window with no opening instant cannot have been preceded by anything.
///
/// Read with [`WindowReach::Inside`], only the events at or after `from`
/// count: the Scope in force when the window opens is the latest event before
/// it across every part of a split entity (clause 5.7.4.4), which this part
/// does not decide alone.
///
/// `branch` is the branch the supersession probe reads the scope history of,
/// so a temporal query over a Snapshot branch tests that branch's own events,
/// and one over the live branch its queued Scope entries too. `of` is whose
/// history the event under [`ScopeEvent`] was read from, so the probe names
/// the same Evolution by what that source projects.
#[must_use]
pub fn overlaps_window(axis: ScopeHistory, interval: TimeInterval, reach: WindowReach, branch: &HistoryBranch, of: &HistoryOf) -> Condition {
    let opening = |from| match reach {
        WindowReach::Inside => Expr::col((ScopeEvent, axis)).gte(from),
        WindowReach::InsideAndInEffect => not_superseded_by(Expr::val(from), axis, branch, of),
    };
    let mut condition = Condition::all();
    match interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(to) => condition = condition.add(Expr::col((ScopeEvent, axis)).lt(to)),
        TimeInterval::After(from) => condition = condition.add(opening(from)),
        TimeInterval::Between { from, to } => {
            condition = condition.add(Expr::col((ScopeEvent, axis)).lt(to)).add(opening(from));
        }
    }
    condition
}

/// Whether no event of the same entity, later on `axis`, falls at or before
/// `instant`.
///
/// Clause 4.18 gives an Entity at most one instance of the scope Property, so
/// any later event replaces the whole value and ends the interval the outer
/// event opened — whatever paths it names, and whether it names any at all. The
/// comparison against the outer event is strict, which is what stops the
/// several rows one multi-path write lands at a single instant from ending each
/// other's interval and collapsing it to zero length.
fn not_superseded_by(instant: SimpleExpr, axis: ScopeHistory, branch: &HistoryBranch, of: &HistoryOf) -> SimpleExpr {
    let mut probe = Query::select();
    probe
        .expr(Expr::val(1))
        .from(scope_rows(branch, &of.same_evolution_as(ScopeEvent), SupersedingScopeEvent))
        .and_where(Expr::col((SupersedingScopeEvent, axis)).gt(Expr::col((ScopeEvent, axis))))
        .and_where(Expr::col((SupersedingScopeEvent, axis)).lte(instant));
    Expr::exists(probe.take()).not()
}

#[cfg(test)]
mod tests {
    use crate::{
        history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
        scope_validity::{ScopeEvent, overlaps_window, positioned_on_axis, scope_history_axis},
        window_reach::WindowReach,
    };
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::ScopeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, Iden, PostgresQueryBuilder, Query};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .expr(Expr::val(1))
            .from_as(ScopeHistory::Table, ScopeEvent)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn each_time_property_positions_a_scope_event_on_its_own_column() {
        let expected = [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ];
        for (timeproperty, column) in expected {
            pretty_assertions::assert_eq!(Iden::to_string(&scope_history_axis(timeproperty)), column);
        }
    }

    /// Clause 4.18: a scope set before the window and never removed is valid
    /// throughout it, so a `between` window never tests the event instant for
    /// membership of the window. Annex C clause C.5.16's Vehicle B9211 is
    /// exactly that entity.
    #[test]
    fn a_between_window_admits_an_event_that_started_before_it() {
        let sql = render(overlaps_window(
            ScopeHistory::ObservedAt,
            TimeInterval::Between {
                from: instant("2018-08-01T12:00:00Z"),
                to: instant("2018-08-01T13:00:00Z"),
            },
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live(),
            &HistoryOf::EveryEntity,
        ));
        assert!(sql.contains("\"sh\".\"observed_at\" < "), "{sql}");
        assert!(!sql.contains("\"sh\".\"observed_at\" >= "), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn an_unbounded_window_constrains_nothing() {
        let sql = render(overlaps_window(
            ScopeHistory::ObservedAt,
            TimeInterval::Unbounded,
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live(),
            &HistoryOf::EveryEntity,
        ));
        assert!(!sql.contains("\"sh\".\"observed_at\""), "{sql}");
        assert!(!sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn a_before_window_bounds_only_the_start_of_the_interval() {
        let sql = render(overlaps_window(
            ScopeHistory::ObservedAt,
            TimeInterval::Before(instant("2018-08-01T13:00:00Z")),
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live(),
            &HistoryOf::EveryEntity,
        ));
        assert!(sql.contains("\"sh\".\"observed_at\" < "), "{sql}");
        assert!(!sql.contains("NOT EXISTS"), "{sql}");
    }

    #[test]
    fn an_after_window_bounds_only_the_end_of_the_interval() {
        let sql = render(overlaps_window(
            ScopeHistory::ObservedAt,
            TimeInterval::After(instant("2018-08-01T12:00:00Z")),
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live(),
            &HistoryOf::EveryEntity,
        ));
        assert!(!sql.contains("\"sh\".\"observed_at\" < "), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    /// Clause 5.7.4.4: read inside only, an event counts when it lies at or
    /// after the window's opening, and no supersession is probed.
    #[test]
    fn an_inside_reach_admits_only_events_inside_the_window() {
        let sql = render(overlaps_window(
            ScopeHistory::ObservedAt,
            TimeInterval::Between {
                from: instant("2018-08-01T12:00:00Z"),
                to: instant("2018-08-01T13:00:00Z"),
            },
            WindowReach::Inside,
            &HistoryBranch::live(),
            &HistoryOf::EveryEntity,
        ));
        assert!(sql.contains("\"sh\".\"observed_at\" >= '2018-08-01 12:00:00"), "{sql}");
        assert!(sql.contains("\"sh\".\"observed_at\" < '2018-08-01 13:00:00"), "{sql}");
        assert!(!sql.contains("NOT EXISTS"), "{sql}");
    }

    /// The supersession probe compares strictly later events and stays inside
    /// the entity the outer event belongs to.
    #[test]
    fn supersession_is_strict_and_confined_to_one_entity() {
        let sql = render(overlaps_window(
            ScopeHistory::ObservedAt,
            TimeInterval::After(instant("2018-08-01T12:00:00Z")),
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live(),
            &HistoryOf::EveryEntity,
        ));
        assert!(sql.contains("\"sh_next\".\"observed_at\" > \"sh\".\"observed_at\""), "{sql}");
        assert!(!sql.contains("\"sh_next\".\"observed_at\" >= \"sh\".\"observed_at\""), "{sql}");
        assert!(sql.contains("\"re\".\"id\" = \"sh\".\"entity_id\""), "{sql}");
        assert!(sql.contains("\"pe\".\"entity_id\" = \"sh\".\"entity_id\""), "{sql}");
    }

    /// An event read for an Evolution by key carries the key and no
    /// identifier, so over recorded rows alone the supersession probe names
    /// the same Evolution by the key, and over a queue by the identifier.
    #[test]
    fn supersession_names_the_evolution_by_what_the_event_source_projects() {
        let of = HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))),
        });
        let after = TimeInterval::After(instant("2018-08-01T12:00:00Z"));
        let recorded = render(overlaps_window(
            ScopeHistory::ObservedAt,
            after,
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live_recorded(),
            &of,
        ));
        assert!(recorded.contains(r#""rsh"."temporal_entity_key" = "sh"."temporal_entity_key""#), "{recorded}");
        assert!(!recorded.contains("temporal_entities") && !recorded.contains("entity_id"), "{recorded}");
        let queued = render(overlaps_window(
            ScopeHistory::ObservedAt,
            after,
            WindowReach::InsideAndInEffect,
            &HistoryBranch::live(),
            &of,
        ));
        assert!(queued.contains(r#""re"."id" = "sh"."entity_id""#), "{queued}");
        assert!(queued.contains(r#""pe"."entity_id" = "sh"."entity_id""#), "{queued}");
    }

    #[test]
    fn an_event_carrying_no_instant_on_the_axis_is_excluded() {
        let sql = render(Condition::all().add(positioned_on_axis(ScopeHistory::ObservedAt)));
        assert!(sql.contains("\"sh\".\"observed_at\" IS NOT NULL"), "{sql}");
    }
}
