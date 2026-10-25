//! The clause 4.18 intervals over which a `scopeQ` held on one Entity.
//!
//! Annex C clause C.5.16 includes only the "Property values ... that have been
//! observed after the Scope has become valid", so a temporal query with a
//! `scopeQ` admits an attribute instance only while the query held on the
//! Entity's Scope at the instant the instance is positioned on. Clause 4.18
//! keeps a Scope valid from the time it is set until a later write replaces or
//! removes it, so the Scope in force at an instant is the paths of the
//! Entity's last scope event at or before it, and it changes only at the
//! Entity's scope events.
//!
//! The query is therefore judged once per instant an event is positioned on,
//! never once per instance: the events are grouped by instant into the paths
//! they hold, each group is judged, consecutive groups judged alike merge, and
//! each merged interval runs until the next one starts. An instance is admitted
//! when it falls inside an interval the query held over, a semi-join against a
//! list as long as the number of times the judgement changed, however many
//! events restated the same Scope.

use crate::{
    history_source::{HistoryBranch, HistoryOf, scope_rows::scope_rows},
    scope_predicate::{ScopeTarget, condition},
    scope_validity::{ScopeEvent, positioned_on_axis, scope_history_axis},
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use andromeda_postgresql_schema::temporal::ScopeHistory;
use andromeda_postgresql_seaquery::{
    aggregate::{array_agg_distinct_filter, lag, lead},
    cast::cast,
    pg_type::PostgresType,
};
use sea_query::{Expr, ExprTrait, Func, Iden, Order, Query, SelectStatement, SimpleExpr, WindowStatement};

/// The relations the interval reading nests, innermost first.
#[derive(Iden, Clone, Copy)]
enum ScopeSpan {
    /// The Entity's Scope from each instant one of its events is positioned on.
    #[iden = "scope_states"]
    States,
    /// Each state beside whether the query held over it.
    #[iden = "judged_states"]
    Judged,
    /// Each judged state beside the judgement of the state before it.
    #[iden = "marked_states"]
    Marked,
    /// The states the judgement changes at, each running until the next.
    #[iden = "scope_intervals"]
    Intervals,
}

/// The columns of the interval reading.
#[derive(Iden, Clone, Copy)]
enum ScopeSpanColumn {
    /// The instant a state or an interval starts at.
    Starts,
    /// The instant the next interval starts at, NULL for the last interval,
    /// which no later event ends.
    Ends,
    /// The scope paths the Entity holds over a state, empty after a removal.
    Paths,
    /// Whether the query held over a state.
    InScope,
    /// Whether the query held over the state before, NULL for the first state.
    WasInScope,
}

/// Whether the `scopeQ` held on the Scope of the Entity `of` names at
/// `instant`, the Scope read from the scope history of `branch` and positioned
/// on the `timeproperty` axis.
///
/// An instant before the Entity's first event, or one that is NULL, lies in no
/// interval, so the query does not hold there.
#[must_use]
pub fn held_at(scope_q: &ScopeQuery, of: &HistoryOf, timeproperty: TimeProp, instant: SimpleExpr, branch: &HistoryBranch) -> SimpleExpr {
    let column = |name| Expr::col((ScopeSpan::Intervals, name));
    let mut probe = Query::select();
    probe
        .expr(Expr::val(1))
        .from_subquery(scope_intervals(scope_q, of, timeproperty, branch), ScopeSpan::Intervals)
        .and_where(column(ScopeSpanColumn::InScope))
        // clone: the instant bounds the interval on both sides.
        .and_where(column(ScopeSpanColumn::Starts).lte(instant.clone()))
        .and_where(column(ScopeSpanColumn::Ends).is_null().or(instant.lt(column(ScopeSpanColumn::Ends))));
    Expr::exists(probe.take())
}

/// The maximal intervals over which the query's judgement of the Entity's
/// Scope stays the same, each beside that judgement.
fn scope_intervals(scope_q: &ScopeQuery, of: &HistoryOf, timeproperty: TimeProp, branch: &HistoryBranch) -> SelectStatement {
    let column = |name| Expr::col((ScopeSpan::Marked, name));
    let mut intervals = Query::select();
    intervals
        .column((ScopeSpan::Marked, ScopeSpanColumn::Starts))
        .column((ScopeSpan::Marked, ScopeSpanColumn::InScope))
        .expr_window_as(lead(column(ScopeSpanColumn::Starts)), in_start_order(ScopeSpan::Marked), ScopeSpanColumn::Ends)
        .from_subquery(marked_states(scope_q, of, timeproperty, branch), ScopeSpan::Marked)
        .and_where(
            column(ScopeSpanColumn::WasInScope)
                .is_null()
                .or(column(ScopeSpanColumn::WasInScope).ne(column(ScopeSpanColumn::InScope))),
        );
    intervals.take()
}

/// Each judged state beside the judgement of the state before it, which tells
/// a state that starts an interval from one that extends it.
fn marked_states(scope_q: &ScopeQuery, of: &HistoryOf, timeproperty: TimeProp, branch: &HistoryBranch) -> SelectStatement {
    let mut marked = Query::select();
    marked
        .column((ScopeSpan::Judged, ScopeSpanColumn::Starts))
        .column((ScopeSpan::Judged, ScopeSpanColumn::InScope))
        .expr_window_as(
            lag(Expr::col((ScopeSpan::Judged, ScopeSpanColumn::InScope))),
            in_start_order(ScopeSpan::Judged),
            ScopeSpanColumn::WasInScope,
        )
        .from_subquery(judged_states(scope_q, of, timeproperty, branch), ScopeSpan::Judged);
    marked.take()
}

/// Each state beside whether the query held over it.
///
/// The query is judged on the paths of the state together, so a conjunction is
/// met by two paths one multi-path write set at the same instant.
fn judged_states(scope_q: &ScopeQuery, of: &HistoryOf, timeproperty: TimeProp, branch: &HistoryBranch) -> SelectStatement {
    let in_scope = condition(scope_q, &ScopeTarget::paths((ScopeSpan::States, ScopeSpanColumn::Paths)));
    let mut judged = Query::select();
    judged
        .column((ScopeSpan::States, ScopeSpanColumn::Starts))
        .expr_as(SimpleExpr::from(in_scope), ScopeSpanColumn::InScope)
        .from_subquery(scope_states(of, timeproperty, branch), ScopeSpan::States);
    judged.take()
}

/// The Entity's Scope from each instant one of its events is positioned on:
/// the paths of every event at that instant.
///
/// A removal names no path, so the state it starts holds none; the paths are
/// never NULL, so every judgement is true or false and two states judged alike
/// always merge.
fn scope_states(of: &HistoryOf, timeproperty: TimeProp, branch: &HistoryBranch) -> SelectStatement {
    let axis = scope_history_axis(timeproperty);
    let path = || Expr::col((ScopeEvent, ScopeHistory::ScopePath));
    let mut states = Query::select();
    states
        .expr_as(Expr::col((ScopeEvent, axis)), ScopeSpanColumn::Starts)
        .expr_as(
            Func::coalesce([
                array_agg_distinct_filter(path(), path().is_not_null()),
                cast(Expr::val("{}"), PostgresType::LtreeArray),
            ]),
            ScopeSpanColumn::Paths,
        )
        .from(scope_rows(branch, of, ScopeEvent))
        .and_where(positioned_on_axis(axis))
        .group_by_col((ScopeEvent, axis));
    states.take()
}

/// The order of the states or intervals of `relation`, earliest first.
fn in_start_order(relation: ScopeSpan) -> WindowStatement {
    let mut window = WindowStatement::new();
    window.order_by((relation, ScopeSpanColumn::Starts), Order::Asc);
    window
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        history_source::{HistoryBranch, HistoryOf},
        scope_intervals::held_at,
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn rendered(raw: &str, timeproperty: TimeProp) -> String {
        let scope_q = ScopeQuery::parse(raw).expect("valid scope query");
        let held = held_at(
            &scope_q,
            &HistoryOf::Entity(Expr::val("urn:ngsi-ld:Vehicle:1")),
            timeproperty,
            Expr::col((Alias::new("ah"), Alias::new("observed_at"))),
            &HistoryBranch::live_recorded(),
        );
        Query::select().expr(held).to_owned().to_string(PostgresQueryBuilder)
    }

    /// The Entity's events are read once, for the Entity the statement names,
    /// and no probe asks per instance which event superseded which.
    #[test]
    fn the_scope_events_are_read_once_for_the_entity() {
        let sql = rendered("/Madrid/Centro", TimeProp::Observed);
        assert_eq!(sql.matches(r#"FROM "scope_history""#).count(), 1, "{sql}");
        assert!(sql.contains(r#""re"."id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        assert!(!sql.contains("sh_next"), "{sql}");
        assert!(!sql.contains(r#""ah"."entity_id""#), "the intervals do not depend on the instance: {sql}");
    }

    /// Clause 4.18: every event at one instant makes up the Scope from then on,
    /// a removal contributing no path.
    #[test]
    fn the_events_at_one_instant_make_up_one_state() {
        let sql = rendered("/Madrid/Centro", TimeProp::Observed);
        assert!(
            sql.contains(r#"COALESCE(array_agg(DISTINCT "sh"."scope_path") FILTER (WHERE "sh"."scope_path" IS NOT NULL), '{}'::ltree[]) AS "paths""#),
            "{sql}"
        );
        assert!(sql.contains(r#"WHERE "sh"."observed_at" IS NOT NULL GROUP BY "sh"."observed_at""#), "{sql}");
    }

    /// The query is judged on the paths of each state together, so the two
    /// requirements of a conjunction may be met by two paths of one state.
    #[test]
    fn a_conjunction_is_judged_on_the_paths_of_one_state() {
        let sql = rendered("(/A;/B)", TimeProp::Observed);
        assert!(
            sql.contains(r#"("scope_states"."paths" ~ ('A'::lquery)) AND ("scope_states"."paths" ~ ('B'::lquery))"#),
            "{sql}"
        );
    }

    /// A state judged like the one before it extends that state's interval, so
    /// restating the Scope in force starts no interval.
    #[test]
    fn consecutive_states_judged_alike_merge_into_one_interval() {
        let sql = rendered("/Madrid/Centro", TimeProp::Observed);
        assert!(
            sql.contains(r#"LAG("judged_states"."in_scope") OVER (  ORDER BY "judged_states"."starts" ASC ) AS "was_in_scope""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"WHERE "marked_states"."was_in_scope" IS NULL OR "marked_states"."was_in_scope" <> "marked_states"."in_scope""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEAD("marked_states"."starts") OVER (  ORDER BY "marked_states"."starts" ASC ) AS "ends""#),
            "{sql}"
        );
    }

    /// Annex C clause C.5.16: an instance is admitted from the instant its
    /// interval starts, inclusive, until the next one starts, exclusive, the
    /// last interval left open.
    #[test]
    fn an_instance_is_admitted_inside_an_interval_the_query_held_over() {
        let sql = rendered("/Madrid/Centro", TimeProp::Observed);
        assert!(
            sql.ends_with(r#"AS "scope_intervals" WHERE "scope_intervals"."in_scope" AND "scope_intervals"."starts" <= "ah"."observed_at" AND ("scope_intervals"."ends" IS NULL OR "ah"."observed_at" < "scope_intervals"."ends"))"#),
            "{sql}"
        );
    }

    /// Clause 5.2.21: the states are positioned on the axis the query asks for.
    #[test]
    fn the_states_are_positioned_on_the_requested_axis() {
        let sql = rendered("/Madrid/Centro", TimeProp::Created);
        assert!(sql.contains(r#""sh"."created_at" AS "starts""#), "{sql}");
        assert!(sql.contains(r#"GROUP BY "sh"."created_at""#), "{sql}");
        assert!(!sql.contains(r#""sh"."observed_at" AS"#), "{sql}");
    }
}
