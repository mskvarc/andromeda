//! The candidates of a temporal window (clause 4.11): the Evolutions holding
//! an instance of a concerned attribute whose instant on the window's axis
//! lies inside it, read from the history rows the window names.
//!
//! Clause 5.7.4.4 step S1 keeps an Entity any of whose instances of the
//! concerned attributes, or of every attribute when the request names none,
//! falls in the window, a test on history rows alone, so the recorded rows
//! inside the window name exactly the recorded Evolutions S1 keeps. The read
//! takes rows, not Evolutions, and bounds them, so a window many rows answer
//! walks. Clause 5.7.4.4 refuses a query without a temporal query, so every
//! query takes this step when no other applies, unless each of its selectors
//! names identifiers, which the identifier index answers already. The window
//! match stays in the residual: it judges the queued entries and the rows a
//! queued entry replaces, which no index read decides.

use crate::{
    alias::{Rh, Rs},
    axis::axis_column,
    stage1::{evolution_walk::concerned_attribute_keys, queued_candidates::queued_candidates},
};
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, window::TemporalWindow};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::query_names::QueryNames,
    history_source::{HistoryBranch, PendingEntries},
    time_interval_predicate::within_interval,
};
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Expr, ExprTrait, JoinType, Query, SelectStatement, UnionType};

/// How many Evolutions a windowed page walks before it reads the candidates:
/// a window most Evolutions answer fills the page within them.
pub(super) const WINDOW_SHORT_WALK: u32 = 200;

/// The most history rows a page reads before it walks instead.
pub(super) const WINDOW_CANDIDATE_BOUND: u64 = 10_000;

/// Whose instances a window read takes.
#[derive(Debug, Clone)]
enum ConcernedAttributes {
    /// Every attribute's: the request names none.
    Every,
    /// Those of the attributes the request names, by dictionary key; an
    /// attribute the dictionary never keyed has no history and no key.
    Keyed(Vec<AttributeNameKey>),
}

/// The history read a bounded temporal window admits.
#[derive(Debug, Clone)]
pub(super) struct WindowRead<'q> {
    /// The window: its axis and its interval.
    window: &'q TemporalWindow,
    /// Whose instances it reads.
    concerned: ConcernedAttributes,
}

impl<'q> WindowRead<'q> {
    /// The window read of `compiled`: `None` for an unbounded window, which
    /// every Evolution holding a stream of a concerned attribute meets, and
    /// for selectors that each name identifiers.
    pub(super) fn of(compiled: &'q CompiledTemporalQuery, names: &QueryNames) -> Option<WindowRead<'q>> {
        if !compiled.window.interval.is_bounded() {
            return None;
        }
        let selectors = &compiled.entities.selectors;
        if !selectors.is_empty() && selectors.iter().all(|selector| !selector.ids.is_empty()) {
            return None;
        }
        let concerned = if compiled.instances.attr_filter.is_empty() {
            ConcernedAttributes::Every
        } else {
            ConcernedAttributes::Keyed(concerned_attribute_keys(compiled, &names.attributes))
        };
        Some(WindowRead {
            window: &compiled.window,
            concerned,
        })
    }

    /// The Evolutions of `branch` with a recorded row of a concerned
    /// attribute inside the window, once for each such row: their surrogate
    /// keys over recorded rows alone; where the branch reads its queue, their
    /// identifiers beside every queued Entity, whose queued entries no index
    /// holds.
    pub(super) fn candidates(&self, branch: &HistoryBranch) -> SelectStatement {
        let column = match branch.pending() {
            PendingEntries::Absent => TemporalAttributeStreams::TemporalEntityKey,
            PendingEntries::Unioned => TemporalAttributeStreams::Id,
        };
        let mut candidates = Query::select();
        candidates
            .column((Rs, column))
            .from(qualified(branch.schema(), AttributeHistory::Table).alias(Rh))
            .join_as(
                JoinType::Join,
                qualified(branch.schema(), TemporalAttributeStreams::Table),
                Rs,
                Expr::col((Rs, TemporalAttributeStreams::StreamKey)).equals((Rh, AttributeHistory::StreamKey)),
            )
            .cond_where(within_interval(&Expr::col((Rh, axis_column(self.window.timeproperty))), &self.window.interval));
        match &self.concerned {
            ConcernedAttributes::Every => {}
            ConcernedAttributes::Keyed(keys) => {
                candidates.and_where(Expr::col((Rs, TemporalAttributeStreams::AttributeNameKey)).is_in(keys.iter().map(|key| key.get())));
            }
        }
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
    fn a_window_over_a_type_walk_reads_the_axis_rows() {
        let stage1 = recorded_stage1(&modified_after());
        assert_eq!(stage1.planning, StatementPlanning::GatedBranches);
        let (sql, values) = stage1.select.build(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        assert!(
            candidates.starts_with(concat!(
                r#"SELECT "rs"."temporal_entity_key" FROM "attribute_history" AS "rh""#,
                r#" JOIN "temporal_attribute_streams" AS "rs" ON "rs"."stream_key" = "rh"."stream_key""#,
                r#" WHERE "rh"."modified_at" >= $"#
            )),
            "{candidates}"
        );
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
    }

    #[test]
    fn each_axis_reads_its_own_column() {
        let axes = [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ];
        for (timeproperty, column) in axes {
            let candidates = candidates_of(&windowed(timeproperty, TimeInterval::After(instant("2026-10-18T03:00:00Z"))));
            assert!(candidates.contains(&format!(r#""rh"."{column}" >= "#)), "{timeproperty}: {candidates}");
            for (_, other) in axes.iter().filter(|(_, other)| *other != column) {
                assert!(!candidates.contains(other), "{timeproperty}: {candidates}");
            }
            assert!(!candidates.contains("recorded_at"), "{timeproperty}: {candidates}");
        }
    }

    #[test]
    fn between_and_before_windows_bound_both_or_one_side() {
        let between = candidates_of(&windowed(
            TimeProp::Modified,
            TimeInterval::Between {
                from: instant("2026-10-18T03:00:00Z"),
                to: instant("2026-10-19T03:00:00Z"),
            },
        ));
        assert!(
            between.contains(r#""rh"."modified_at" >= '2026-10-18 03:00:00.000000 +00:00' AND "rh"."modified_at" < '2026-10-19 03:00:00.000000 +00:00'"#),
            "{between}"
        );
        let before = candidates_of(&windowed(TimeProp::Modified, TimeInterval::Before(instant("2026-10-18T03:00:00Z"))));
        assert!(before.contains(r#""rh"."modified_at" < '2026-10-18 03:00:00.000000 +00:00'"#), "{before}");
        assert!(!before.contains(">="), "{before}");
    }

    #[test]
    fn the_concerned_attributes_narrow_the_window_candidates() {
        let mut compiled = modified_after();
        compiled.instances.attr_filter = vec![iri("https://uri.etsi.org/ngsi-ld/default-context/temperature")];
        let (sql, values) = recorded_stage1(&compiled).select.build(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        let (_, keys) = candidates
            .split_once(r#" AND "rs"."attribute_name_key" IN ($"#)
            .unwrap_or_else(|| panic!("the concerned keys: {candidates}"));
        let position: usize = keys
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("a placeholder");
        assert_eq!(values.0.get(position - 1), Some(&Value::Int(Some(1))), "{sql}");
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
            count.contains(r#""ah"."modified_at" >= '2026-10-18 03:00:00.000000 +00:00' AND "ah"."temporal_entity_key" = "e"."temporal_entity_key""#),
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

    #[test]
    fn the_window_match_stays_the_residual() {
        let sql = recorded_stage1(&modified_after()).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches
                .indexed
                .contains(r#""temporal_entity_key" IN (SELECT "temporal_entity_key" FROM "candidates")"#),
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
            branches.candidates.starts_with(r#"SELECT "rs"."id" FROM "attribute_history" AS "rh" JOIN"#),
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
            candidates.contains(r#"FROM "snap_7"."attribute_history" AS "rh" JOIN "snap_7"."temporal_attribute_streams" AS "rs""#),
            "{candidates}"
        );
    }
}
