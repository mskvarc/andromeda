//! The candidates of a temporal window (clause 4.11) that opens at an
//! instant: the Evolutions whose activity horizon is at or after it, read from
//! the horizon index of the catalogue.
//!
//! Clause 5.7.4.4 step S1 keeps an Entity any of whose instances of the
//! concerned attributes, or of every attribute when the request names none,
//! falls in the window, and an instance falls in an `after` or `between`
//! window only at or after its start. The horizon of an Evolution bounds every
//! instant its instances hold on every axis, so an Evolution S1 keeps has a
//! horizon at or after the start: the candidates hold every Evolution the
//! window keeps, and only Evolutions written within the hour before the start
//! or later. A `before` window reaches back without limit, so no horizon
//! narrows it and it takes no step. Clause 5.7.4.4 refuses a query without a
//! temporal query, so every query takes this step when no other applies,
//! unless each of its selectors names identifiers, which the identifier index
//! answers already. The window match stays in the residual: it decides which
//! candidates S1 keeps, and judges the queued entries and the rows a queued
//! entry replaces, which no horizon covers.

use crate::stage1::queued_candidates::queued_candidates;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::history_source::{HistoryBranch, PendingEntries};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::branch::qualified;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Query, SelectStatement, UnionType};

/// How many Evolutions a windowed page walks before it reads the candidates:
/// a window most Evolutions answer fills the page within them.
pub(super) const WINDOW_SHORT_WALK: u32 = 200;

/// The most candidate Evolutions a page reads before it walks instead.
pub(super) const WINDOW_CANDIDATE_BOUND: u64 = 10_000;

/// The most Evolutions a windowed page walks in place of reading the
/// candidates, when the matches the short walk found project the page full
/// within them.
///
/// The candidates are every Evolution written since the hour before the
/// window's start, up to the bound, however few of them the window keeps;
/// a window dense enough to fill the page within this walk finds its page
/// for fewer probes than reading them costs.
pub(super) const WINDOW_DENSE_WALK: u32 = 1_500;

/// The horizon read a temporal window opening at an instant admits.
#[derive(Debug, Clone, Copy)]
pub(super) struct WindowRead {
    /// The instant the window opens at: every instance it keeps lies at or
    /// after it.
    opening: DateTime<Utc>,
}

impl WindowRead {
    /// The window read of `compiled`: `None` for a window that reaches back
    /// without limit, an unbounded or a `before` one, and for selectors that
    /// each name identifiers.
    pub(super) fn of(compiled: &CompiledTemporalQuery) -> Option<WindowRead> {
        let opening = *compiled.window.interval.opening()?;
        let selectors = &compiled.entities.selectors;
        if !selectors.is_empty() && selectors.iter().all(|selector| !selector.ids.is_empty()) {
            return None;
        }
        Some(WindowRead { opening })
    }

    /// The Evolutions of `branch` whose horizon is at or after the window's
    /// start: their surrogate keys over recorded rows alone; where the branch
    /// reads its queue, their identifiers beside every queued Entity, whose
    /// queued entries no horizon covers yet.
    pub(super) fn candidates(&self, branch: &HistoryBranch) -> SelectStatement {
        let column = match branch.pending() {
            PendingEntries::Absent => Entities::EntityKey,
            PendingEntries::Unioned => Entities::Id,
        };
        let mut candidates = Query::select();
        candidates
            .column((Entities::Table, column))
            .from(qualified(branch.schema(), Entities::Table))
            .and_where(Expr::col((Entities::Table, Entities::EvolutionActiveUntil)).gte(self.opening));
        match branch.pending() {
            PendingEntries::Absent => {}
            PendingEntries::Unioned => {
                candidates.union(UnionType::All, queued_candidates(branch));
            }
        }
        candidates
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stage1::query_fixture::{PageBranches, ids, instant, iri, recorded_stage1, sensors, stage1_on};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelector,
        page_window::PageWindow,
        temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Value};
    use strum::IntoEnumIterator;

    /// A query of the `Sensor` type over `interval` on `timeproperty`.
    fn windowed(timeproperty: TimeProp, interval: TimeInterval) -> CompiledTemporalQuery {
        let mut compiled = sensors();
        compiled.window.timeproperty = timeproperty;
        compiled.window.interval = interval;
        compiled
    }

    /// A query of the `Sensor` type over `timerel=after` on `modifiedAt`.
    fn modified_after() -> CompiledTemporalQuery {
        windowed(TimeProp::Modified, TimeInterval::After(instant("2026-10-18T03:00:00Z")))
    }

    /// The candidate read of the gated page of `compiled` over the `Sensor`
    /// walk, rendered with its values inlined.
    fn candidates_of(compiled: &CompiledTemporalQuery) -> String {
        let sql = recorded_stage1(compiled).select.to_string(PostgresQueryBuilder);
        PageBranches::of(&sql).candidates.to_owned()
    }

    #[test]
    fn a_window_over_a_type_walk_reads_the_horizons_at_or_after_its_start() {
        let stage1 = recorded_stage1(&modified_after());
        assert_eq!(stage1.planning, StatementPlanning::GatedBranches);
        let (sql, values) = stage1.select.build(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        assert!(
            candidates.starts_with(r#"SELECT "entities"."entity_key" FROM "entities" WHERE "entities"."evolution_active_until" >= $"#),
            "{candidates}"
        );
        assert!(!candidates.contains("attribute_history"), "{candidates}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
    }

    /// The horizon bounds the instants of every axis at once, so the axis
    /// changes only the residual window match.
    #[test]
    fn every_axis_reads_the_same_horizons() {
        let candidates: Vec<String> = TimeProp::iter()
            .map(|timeproperty| candidates_of(&windowed(timeproperty, TimeInterval::After(instant("2026-10-18T03:00:00Z")))))
            .collect();
        assert_eq!(
            candidates.first().map(String::as_str),
            Some(r#"SELECT "entities"."entity_key" FROM "entities" WHERE "entities"."evolution_active_until" >= '2026-10-18 03:00:00.000000 +00:00'"#)
        );
        assert!(candidates.windows(2).all(|pair| pair[0] == pair[1]), "{candidates:?}");
    }

    #[test]
    fn a_between_window_reads_the_horizons_from_its_start() {
        let between = candidates_of(&windowed(
            TimeProp::Modified,
            TimeInterval::Between {
                from: instant("2026-10-18T03:00:00Z"),
                to: instant("2026-10-19T03:00:00Z"),
            },
        ));
        assert_eq!(
            between,
            r#"SELECT "entities"."entity_key" FROM "entities" WHERE "entities"."evolution_active_until" >= '2026-10-18 03:00:00.000000 +00:00'"#
        );
    }

    /// A `before` window reaches back without limit, which no horizon bounds.
    #[test]
    fn a_before_window_has_no_window_step() {
        let stage1 = recorded_stage1(&windowed(TimeProp::Modified, TimeInterval::Before(instant("2026-10-18T03:00:00Z"))));
        let sql = stage1.select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("candidates"), "{sql}");
        assert!(!sql.contains("evolution_active_until"), "{sql}");
    }

    /// The horizon covers every attribute of the Evolution, so the concerned
    /// attributes narrow only the residual window match.
    #[test]
    fn the_concerned_attributes_leave_the_candidates_alone() {
        let mut compiled = modified_after();
        compiled.instances.attr_filter = vec![iri("https://uri.etsi.org/ngsi-ld/default-context/temperature")];
        assert_eq!(candidates_of(&compiled), candidates_of(&modified_after()));
    }

    #[test]
    fn an_unbounded_window_has_no_window_step() {
        let compiled = windowed(TimeProp::Modified, TimeInterval::Unbounded);
        let stage1 = recorded_stage1(&compiled);
        let sql = stage1.select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("candidates"), "{sql}");
        assert_eq!(stage1.planning, StatementPlanning::Cached);
    }

    #[test]
    fn a_selector_naming_identifiers_keeps_the_plain_walk() {
        let mut compiled = modified_after();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Sensor:000001"])).build()];
        let stage1 = recorded_stage1(&compiled);
        let sql = stage1.select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("candidates"), "{sql}");
        assert_eq!(stage1.planning, StatementPlanning::Cached);
    }

    /// Clause 6.3.13 counts every match, which the window match's semi-join
    /// over the time indexes reads already.
    #[test]
    fn a_counted_window_keeps_its_semi_join_count() {
        let stage1 = recorded_stage1(&modified_after());
        assert_eq!(stage1.count_planning, StatementPlanning::Cached);
        let count = stage1.count.expect("count requested").to_string(PostgresQueryBuilder);
        assert!(!count.contains("candidates"), "{count}");
        assert!(count.starts_with(r#"SELECT COUNT("e"."id") FROM"#), "{count}");
        assert!(
            count.contains(r#""ah"."modified_at" >= '2026-10-18 03:00:00.000000 +00:00' AND "ah"."entity_key" = "e"."entity_key""#),
            "{count}"
        );
    }

    #[test]
    fn a_page_of_one_takes_the_window_step() {
        let sql = stage1_on(&HistoryBranch::live_recorded(), &modified_after(), &PageWindow::offset(1, 0))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(branches.short.contains("LIMIT 200 OFFSET 0"), "{}", branches.short);
        assert!(branches.walk.contains(r#"(SELECT COUNT(*) FROM "candidates") > 10000"#), "{}", branches.walk);
    }

    /// A page of 20 behind a short walk of 200 is projected full within the
    /// 1,500-Evolution dense walk once the short walk finds 3 matches (20 *
    /// 200 / 1,500, rounded up).
    #[test]
    fn a_short_walk_projecting_the_page_full_sends_it_to_the_walk_unread() {
        let sql = recorded_stage1(&modified_after()).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        let filled = r#"CASE WHEN ((SELECT COUNT(*) FROM "short_walk") >= 20) THEN FALSE WHEN ((SELECT COUNT(*) FROM "short_walk") >= 3)"#;
        assert!(
            branches
                .walk
                .contains(&format!(r#"{filled} THEN TRUE ELSE (SELECT COUNT(*) FROM "candidates") > 10000 END"#)),
            "{}",
            branches.walk
        );
        assert!(
            branches
                .indexed
                .contains(&format!(r#"{filled} THEN FALSE ELSE (SELECT COUNT(*) FROM "candidates") <= 10000 END"#)),
            "{}",
            branches.indexed
        );
    }

    #[test]
    fn a_page_beyond_the_short_walk_projects_nothing() {
        let sql = stage1_on(&HistoryBranch::live_recorded(), &modified_after(), &PageWindow::offset(20, 250))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(branches.short.contains("LIMIT 0 OFFSET 0"), "{}", branches.short);
        assert!(!branches.walk.contains("CASE"), "{}", branches.walk);
        assert!(
            branches
                .walk
                .contains(r#"(SELECT COUNT(*) FROM "short_walk") < 270 AND (SELECT COUNT(*) FROM "candidates") > 10000"#),
            "{}",
            branches.walk
        );
    }

    #[test]
    fn the_window_match_stays_the_residual() {
        let sql = recorded_stage1(&modified_after()).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches.indexed.contains(r#""entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{}",
            branches.indexed
        );
        for residual in [branches.short, branches.indexed, branches.walk] {
            assert!(residual.contains(r#""ah"."modified_at" >= "#), "{residual}");
        }
    }

    #[test]
    fn a_queued_branch_admits_every_queued_entity_for_its_window() {
        let sql = stage1_on(&HistoryBranch::live(), &modified_after(), &PageWindow::offset(20, 0))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches
                .candidates
                .starts_with(r#"SELECT "entities"."id" FROM "entities" WHERE "entities"."evolution_active_until" >= "#),
            "{}",
            branches.candidates
        );
        assert!(
            branches
                .candidates
                .ends_with(r#" UNION ALL (SELECT "entity_id" FROM "temporal_append_intents")"#),
            "{}",
            branches.candidates
        );
        assert!(
            branches.indexed.contains(r#""e"."id" IN (SELECT "id" FROM "candidates")"#),
            "{}",
            branches.indexed
        );
    }

    #[test]
    fn a_snapshot_reads_its_own_history() {
        let sql = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &modified_after(),
            &PageWindow::offset(20, 0),
        )
        .select
        .to_string(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        assert!(
            candidates.starts_with(r#"SELECT "entities"."entity_key" FROM "snap_7"."entities" WHERE "entities"."evolution_active_until" >= "#),
            "{candidates}"
        );
    }
}
