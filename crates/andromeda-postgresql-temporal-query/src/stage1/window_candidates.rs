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

/// How many Evolutions a windowed page walks before it reads the candidates.
///
/// A window probe costs about 5 µs an Evolution, so a walk that falls short
/// costs 1 to 2 ms. A window one Evolution in five answers fills the page
/// within it in about 1 ms at every size.
pub(super) const WINDOW_SHORT_WALK: u32 = 200;

/// The most candidate Evolutions a page reads before it walks instead.
///
/// Even when the horizons name only Evolutions the window keeps, the
/// bound's candidates cost about 30 ms among 1 000 000 Evolutions. A window
/// spread through identifier order with more of them is sent to the walk
/// unread by the dense projection, so the read stops at the bound mostly for
/// a window whose matches cluster late in identifier order, which the walk
/// then probes Evolution by Evolution. 20 000 such matches take 5 s walked
/// against about 120 ms read in full.
pub(super) const WINDOW_CANDIDATE_BOUND: u64 = 10_000;

/// The most Evolutions a windowed page walks in place of reading the
/// candidates, when the matches the short walk found project the page full
/// within them.
///
/// The candidates are every Evolution written since the hour before the
/// window's start, up to the bound, however few of them the window keeps.
/// The projected walk costs the same at every size, while the candidates a
/// window of a given density yields grow with the selection, so the walk
/// trades a small selection for a large one. Among 1 000 000 Evolutions it
/// finds the page of a window one in a hundred answers in 11 ms against 32 ms
/// for the candidates, and of one in twenty in 4 ms against 11 ms. Among
/// 100 000 the same one in a hundred costs 10 ms walked against 7 ms read.
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
mod tests {
    use crate::stage1::query_fixture::{candidates_of, ids, instant, iri, recorded_stage1, sensors, stage1_on};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{
        expanded_entity_selector::ExpandedEntitySelector,
        page_window::PageWindow,
        temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, SelectStatement};

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

    /// The gated page of `compiled` over the `Sensor` walk.
    fn page_of(compiled: &CompiledTemporalQuery) -> SelectStatement {
        recorded_stage1(compiled).select
    }

    #[test]
    fn a_window_over_a_type_walk_reads_the_horizons_at_or_after_its_start() {
        let stage1 = recorded_stage1(&modified_after());
        assert_eq!(stage1.planning, StatementPlanning::GenericGatedBranches);
        let statement = stage1.select;
        assert_statement_snapshot!(
            "window-candidates-a-window-over-a-type-walk-reads-the-horizons-at-or-after-its-start",
            statement
        );
    }

    /// The horizon bounds the instants of every axis at once, so the axis
    /// changes only the residual window match.
    #[test]
    fn every_axis_reads_the_same_horizons() {
        let after = || TimeInterval::After(instant("2026-10-18T03:00:00Z"));
        let observed = page_of(&windowed(TimeProp::Observed, after()));
        assert_statement_snapshot!("window-candidates-every-axis-reads-the-same-horizons-1", observed);
        let created = page_of(&windowed(TimeProp::Created, after()));
        assert_statement_snapshot!("window-candidates-every-axis-reads-the-same-horizons-2", created);
        let modified = page_of(&windowed(TimeProp::Modified, after()));
        assert_statement_snapshot!("window-candidates-every-axis-reads-the-same-horizons-3", modified);
        let deleted = page_of(&windowed(TimeProp::Deleted, after()));
        assert_statement_snapshot!("window-candidates-every-axis-reads-the-same-horizons-4", deleted);
        let renderings = [observed, created, modified, deleted].map(|statement| statement.to_string(PostgresQueryBuilder));
        let candidates: Vec<&str> = renderings.iter().map(|sql| candidates_of(sql)).collect();
        assert!(candidates.windows(2).all(|pair| pair[0] == pair[1]), "{candidates:?}");
    }

    #[test]
    fn a_between_window_reads_the_horizons_from_its_start() {
        let statement = page_of(&windowed(
            TimeProp::Modified,
            TimeInterval::Between {
                from: instant("2026-10-18T03:00:00Z"),
                to: instant("2026-10-19T03:00:00Z"),
            },
        ));
        assert_statement_snapshot!("window-candidates-a-between-window-reads-the-horizons-from-its-start", statement);
    }

    /// A `before` window reaches back without limit, which no horizon bounds.
    #[test]
    fn a_before_window_has_no_window_step() {
        let statement = page_of(&windowed(TimeProp::Modified, TimeInterval::Before(instant("2026-10-18T03:00:00Z"))));
        assert_statement_snapshot!("window-candidates-a-before-window-has-no-window-step", statement);
    }

    /// The horizon covers every attribute of the Evolution, so the concerned
    /// attributes narrow only the residual window match.
    #[test]
    fn the_concerned_attributes_leave_the_candidates_alone() {
        let mut compiled = modified_after();
        compiled.instances.attr_filter = vec![iri("https://uri.etsi.org/ngsi-ld/default-context/temperature")];
        let narrowed = page_of(&compiled);
        assert_statement_snapshot!("window-candidates-the-concerned-attributes-leave-the-candidates-alone-1", narrowed);
        let plain = page_of(&modified_after());
        assert_statement_snapshot!("window-candidates-the-concerned-attributes-leave-the-candidates-alone-2", plain);
        let (narrowed, plain) = (narrowed.to_string(PostgresQueryBuilder), plain.to_string(PostgresQueryBuilder));
        assert_eq!(candidates_of(&narrowed), candidates_of(&plain));
    }

    #[test]
    fn an_unbounded_window_has_no_window_step() {
        let compiled = windowed(TimeProp::Modified, TimeInterval::Unbounded);
        let stage1 = recorded_stage1(&compiled);
        assert_eq!(stage1.planning, StatementPlanning::Cached);
        let statement = stage1.select;
        assert_statement_snapshot!("window-candidates-an-unbounded-window-has-no-window-step", statement);
    }

    #[test]
    fn a_selector_naming_identifiers_keeps_the_plain_walk() {
        let mut compiled = modified_after();
        compiled.entities.selectors = vec![ExpandedEntitySelector::builder().ids(ids(&["urn:ngsi-ld:Sensor:000001"])).build()];
        let stage1 = recorded_stage1(&compiled);
        assert_eq!(stage1.planning, StatementPlanning::Cached);
        let statement = stage1.select;
        assert_statement_snapshot!("window-candidates-a-selector-naming-identifiers-keeps-the-plain-walk", statement);
    }

    /// Clause 6.3.13 counts every match, which the window match's semi-join
    /// over the time indexes reads already.
    #[test]
    fn a_counted_window_keeps_its_semi_join_count() {
        let stage1 = recorded_stage1(&modified_after());
        assert_eq!(stage1.count_planning, StatementPlanning::Cached);
        let statement = stage1.count.expect("count requested");
        assert_statement_snapshot!("window-candidates-a-counted-window-keeps-its-semi-join-count", statement);
    }

    #[test]
    fn a_page_of_one_takes_the_window_step() {
        let statement = stage1_on(&HistoryBranch::live_recorded(), &modified_after(), &PageWindow::offset(1, 0)).select;
        assert_statement_snapshot!("window-candidates-a-page-of-one-takes-the-window-step", statement);
    }

    /// A page of 20 behind a short walk of 200 is projected full within the
    /// 1,500-Evolution dense walk once the short walk finds 3 matches (20 *
    /// 200 / 1,500, rounded up).
    #[test]
    fn a_short_walk_projecting_the_page_full_sends_it_to_the_walk_unread() {
        let statement = page_of(&modified_after());
        assert_statement_snapshot!("window-candidates-a-short-walk-projecting-the-page-full-sends-it-to-the-walk-unread", statement);
    }

    #[test]
    fn a_page_beyond_the_short_walk_holds_no_short_walk_and_lets_the_candidates_decide() {
        let statement = stage1_on(&HistoryBranch::live_recorded(), &modified_after(), &PageWindow::offset(20, 250)).select;
        assert_statement_snapshot!(
            "window-candidates-a-page-beyond-the-short-walk-holds-no-short-walk-and-lets-the-candidates-decide",
            statement
        );
    }

    #[test]
    fn the_window_match_stays_the_residual() {
        let statement = page_of(&modified_after());
        assert_statement_snapshot!("window-candidates-the-window-match-stays-the-residual", statement);
    }

    #[test]
    fn a_queued_branch_admits_every_queued_entity_for_its_window() {
        let statement = stage1_on(&HistoryBranch::live(), &modified_after(), &PageWindow::offset(20, 0)).select;
        assert_statement_snapshot!("window-candidates-a-queued-branch-admits-every-queued-entity-for-its-window", statement);
    }

    #[test]
    fn a_snapshot_reads_its_own_history() {
        let statement = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &modified_after(),
            &PageWindow::offset(20, 0),
        )
        .select;
        assert_statement_snapshot!("window-candidates-a-snapshot-reads-its-own-history", statement);
    }
}
