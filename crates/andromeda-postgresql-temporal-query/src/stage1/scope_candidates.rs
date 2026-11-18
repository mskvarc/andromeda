//! The candidates of a `scopeQ` (clause 4.19): the Evolutions with a recorded
//! Scope whose path matches a path the query names, read from the partial
//! Scope path index.
//!
//! Clause 4.18 judges a Scope instance by the overlap of its validity interval
//! with the window, and a conjunction asks for several Scope rows of one
//! Entity, neither of which one row test states. An Entity none of whose
//! Scope rows matches any path of the query matches no alternative, whatever
//! its intervals, so the rows matching any path are a superset the per-Entity
//! probe rechecks with the window and the overlap. The read states
//! `value_kind = 1`, the predicate of `idx_scope_history_scope_path`,
//! which the probe never states, so the index serves this read alone. The
//! kind is written as a literal because the page keeps a generic plan, which
//! uses a partial index only when the statement's text implies its predicate
//! and reads every Scope row otherwise.
//!
//! Clause 5.7.4.4 applies the Scope query of a split query only once the
//! parts from the Context Sources are aggregated, so an Entity a source
//! contributed is a candidate whatever its local Scope rows hold.

use crate::stage1::{contributed_candidates::contributed_candidates, queued_candidates::queued_candidates};
use andromeda_ngsi_ld_query::{scope::scope_query::ScopeQuery, temporal::contributed_entities::ContributedEntities};
use andromeda_postgresql_query::{
    history_source::{HistoryBranch, PendingEntries},
    scope_predicate::{ScopeTarget, pattern_condition},
};
use andromeda_postgresql_schema::{current::Entities, temporal::ScopeHistory};
use andromeda_postgresql_seaquery::{branch::qualified, literal::int_literal};
use andromeda_postgresql_stored_values::scope_value_kind::ScopeValueKind;
use sea_query::{Condition, Expr, ExprTrait, JoinType, Query, SelectStatement, UnionType};

/// How many Evolutions a Scope page walks before it reads the candidates.
///
/// A Scope probe costs under a microsecond an Evolution, so a walk that falls
/// short costs a fraction of a millisecond, and a Scope one Evolution in five
/// holds fills the page within it in about half a millisecond at every size.
pub(super) const SCOPE_SHORT_WALK: u32 = 200;

/// The most Scope rows a page reads before it walks instead.
///
/// A row costs about 3 µs to read from the Scope path index and narrow the
/// walk to, so the bound's rows cost about 30 ms among 1 000 000 Evolutions.
/// Past the bound the page walks, which trades the two placements of the
/// matches that no count tells apart. 20 000 matches spread through
/// identifier order cost 9.5 ms with the walk, the read that stopped at the
/// bound included, against 57 ms read in full, while 20 000 matches holding
/// the last identifiers walk in 690 ms against 26 ms read in full.
pub(super) const SCOPE_CANDIDATE_BOUND: u64 = 10_000;

/// The Evolutions of `branch` with a recorded Scope matching a path of
/// `scope_q`, beside every Entity `contributed` names: their surrogate keys
/// over recorded rows alone; where the branch reads its queue, their
/// identifiers beside every queued Entity, whose queued Scope no index holds.
pub(super) fn scope_candidates(branch: &HistoryBranch, scope_q: &ScopeQuery, contributed: &ContributedEntities) -> SelectStatement {
    let mut candidates = recorded_scope_matches(branch, scope_q);
    if let Some(contributed) = contributed_candidates(branch, contributed) {
        candidates.union(UnionType::All, contributed);
    }
    match branch.pending() {
        PendingEntries::Absent => {}
        PendingEntries::Unioned => {
            candidates.union(UnionType::All, queued_candidates(branch));
        }
    }
    candidates
}

/// The Evolutions of `branch` with a recorded Scope row matching any path of
/// `scope_q`: the row's own key over recorded rows alone, the catalogue's
/// identifier where the branch reads its queue.
fn recorded_scope_matches(branch: &HistoryBranch, scope_q: &ScopeQuery) -> SelectStatement {
    let target = ScopeTarget::paths((ScopeHistory::Table, ScopeHistory::ScopePath));
    let paths = scope_q
        .alternatives()
        .flatten()
        .fold(Condition::any(), |paths, pattern| paths.add(pattern_condition(pattern, &target)));
    let mut matches = Query::select();
    matches.from(qualified(branch.schema(), ScopeHistory::Table));
    match branch.pending() {
        PendingEntries::Absent => {
            matches.column((ScopeHistory::Table, ScopeHistory::EntityKey));
        }
        PendingEntries::Unioned => {
            matches.column((Entities::Table, Entities::Id)).join(
                JoinType::Join,
                qualified(branch.schema(), Entities::Table),
                Expr::col((Entities::Table, Entities::EntityKey)).equals((ScopeHistory::Table, ScopeHistory::EntityKey)),
            );
        }
    }
    matches
        .and_where(Expr::col((ScopeHistory::Table, ScopeHistory::ValueKind)).eq(int_literal(i64::from(ScopeValueKind::ScopePath.code()))))
        .cond_where(paths);
    matches
}

#[cfg(test)]
mod tests {
    use crate::{
        split_filter_leaves::build_filter_leaf_truths,
        stage1::query_fixture::{ids, instant, recorded_stage1, scope_query, sensor_names, stage1_on},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{
        page_window::PageWindow,
        temporal::{
            contributed_entities::{ContributedEntities, ContributedRendering},
            window::TimeInterval,
        },
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, SelectStatement};

    /// The page of `scopeQ=<raw>` over the `Sensor` walk.
    fn type_page(raw: &str) -> SelectStatement {
        recorded_stage1(&scope_query(raw)).select
    }

    #[test]
    fn a_scope_query_over_a_type_walk_reads_the_recorded_scope_paths() {
        let stage1 = recorded_stage1(&scope_query("/city/moved1"));
        assert_eq!(stage1.planning, StatementPlanning::GenericGatedBranches);
        let statement = stage1.select;
        assert_statement_snapshot!("scope-candidates-a-scope-query-over-a-type-walk-reads-the-recorded-scope-paths", statement);
    }

    #[test]
    fn the_scope_candidates_state_the_partial_predicate_the_index_holds() {
        let statement = type_page("/city/moved1");
        assert_statement_snapshot!("scope-candidates-the-scope-candidates-state-the-partial-predicate-the-index-holds", statement);
    }

    #[test]
    fn the_per_entity_scope_probe_never_states_the_partial_predicate() {
        let page = type_page("/city/moved1");
        assert_statement_snapshot!("scope-candidates-the-per-entity-scope-probe-never-states-the-partial-predicate-1", page);
        let leaf = build_filter_leaf_truths(
            &HistoryBranch::live_recorded(),
            &scope_query("/city/moved1"),
            &ids(&["urn:ngsi-ld:Sensor:000001"]),
            &sensor_names(),
        )
        .expect("renders");
        assert_statement_snapshot!("scope-candidates-the-per-entity-scope-probe-never-states-the-partial-predicate-2", leaf);
    }

    #[test]
    fn the_scope_path_test_is_the_one_the_probe_renders() {
        let exact = type_page("/city/moved1");
        assert_statement_snapshot!("scope-candidates-the-scope-path-test-is-the-one-the-probe-renders-1", exact);
        let single = type_page("/city/+");
        assert_statement_snapshot!("scope-candidates-the-scope-path-test-is-the-one-the-probe-renders-2", single);
        let multi = type_page("/city/#");
        assert_statement_snapshot!("scope-candidates-the-scope-path-test-is-the-one-the-probe-renders-3", multi);
    }

    #[test]
    fn a_conjunctive_scope_query_reads_rows_matching_any_of_its_paths() {
        let statement = type_page("(/A;/B),/C");
        assert_statement_snapshot!("scope-candidates-a-conjunctive-scope-query-reads-rows-matching-any-of-its-paths", statement);
    }

    #[test]
    fn the_window_and_the_validity_overlap_stay_in_the_probe() {
        let mut compiled = scope_query("/city/moved1");
        compiled.window.timeproperty = TimeProp::Modified;
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        let statement = recorded_stage1(&compiled).select;
        assert_statement_snapshot!("scope-candidates-the-window-and-the-validity-overlap-stay-in-the-probe", statement);
    }

    #[test]
    fn a_scope_page_falls_to_the_walk_past_ten_thousand_rows() {
        let statement = type_page("/city/moved1");
        assert_statement_snapshot!("scope-candidates-a-scope-page-falls-to-the-walk-past-ten-thousand-rows", statement);
    }

    #[test]
    fn a_queued_branch_admits_every_queued_entity_for_its_scope() {
        let statement = stage1_on(&HistoryBranch::live(), &scope_query("/city/moved1"), &PageWindow::offset(20, 0)).select;
        assert_statement_snapshot!("scope-candidates-a-queued-branch-admits-every-queued-entity-for-its-scope", statement);
    }

    #[test]
    fn a_snapshot_reads_its_own_scope_path_index() {
        let statement = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &scope_query("/city/moved1"),
            &PageWindow::offset(20, 0),
        )
        .select;
        assert_statement_snapshot!("scope-candidates-a-snapshot-reads-its-own-scope-path-index", statement);
    }

    /// Clause 5.7.4.4 applies the Scope query of a split query only to the
    /// aggregated Entities, so a contributed Entity is a candidate whatever its
    /// local Scope rows hold, and the residual still exempts it.
    #[test]
    fn a_contributed_entity_is_a_candidate_whatever_its_scope() {
        let mut compiled = scope_query("/city/moved1");
        compiled.contributed = ContributedEntities::new(ids(&["urn:ngsi-ld:Sensor:000009"]), ContributedRendering::Local);
        let recorded = recorded_stage1(&compiled).select;
        assert_statement_snapshot!("scope-candidates-a-contributed-entity-is-a-candidate-whatever-its-scope-1", recorded);
        let queued = stage1_on(&HistoryBranch::live(), &compiled, &PageWindow::offset(20, 0)).select;
        assert_statement_snapshot!("scope-candidates-a-contributed-entity-is-a-candidate-whatever-its-scope-2", queued);
    }

    #[test]
    fn a_counted_scope_query_counts_under_the_count_bound() {
        let stage1 = recorded_stage1(&scope_query("/city/moved1"));
        assert_eq!(stage1.count_planning, StatementPlanning::BoundValues);
        let statement = stage1.count.expect("count requested");
        assert_statement_snapshot!("scope-candidates-a-counted-scope-query-counts-under-the-count-bound", statement);
    }
}
