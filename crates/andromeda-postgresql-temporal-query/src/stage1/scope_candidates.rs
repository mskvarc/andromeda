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
//! which the probe never states, so the index serves this read alone.
//!
//! Clause 5.7.4.4 applies the Scope query of a split query only once the
//! parts from the Context Sources are aggregated, so an Entity a source
//! contributed is a candidate whatever its local Scope rows hold.

use crate::stage1::{contributed_candidates::contributed_candidates, queued_candidates::queued_candidates};
use andromeda_ngsi_ld_query::{scope::query::ScopeQuery, temporal::contributed_entities::ContributedEntities};
use andromeda_postgresql_codes::scope_value_kind::ScopeValueKind;
use andromeda_postgresql_query::{
    history_source::{HistoryBranch, PendingEntries},
    scope_predicate::{ScopeTarget, pattern_condition},
};
use andromeda_postgresql_schema::{current::Entities, temporal::ScopeHistory};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Condition, Expr, ExprTrait, JoinType, Query, SelectStatement, UnionType};

/// How many Evolutions a Scope page walks before it reads the candidates:
/// matches that come early in identifier order fill the page there.
pub(super) const SCOPE_SHORT_WALK: u32 = 200;

/// The most Scope rows a page reads before it walks instead, which the index
/// reads whole in about a millisecond.
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
        .and_where(Expr::col((ScopeHistory::Table, ScopeHistory::ValueKind)).eq(ScopeValueKind::ScopePath.code()))
        .cond_where(paths);
    matches
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        split_filter_leaves::build_filter_leaf_truths,
        stage1::query_fixture::{CountParts, PageBranches, ids, instant, recorded_stage1, scope_query, sensor_names, stage1_on},
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
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Value};

    /// The candidates' test of the recorded Scope path.
    const CANDIDATE_PATH: &str = r#""scope_history"."scope_path" ~ ("#;
    /// The per-Entity probe's test of a Scope event's path.
    const PROBE_PATH: &str = r#""sh"."scope_path" ~ ("#;

    /// The rendered page of `scopeQ=<raw>` over the `Sensor` walk.
    fn type_page(raw: &str) -> String {
        recorded_stage1(&scope_query(raw)).select.to_string(PostgresQueryBuilder)
    }

    /// The `lquery` literal of the first path test `marker` opens in `sql`.
    fn lquery_after<'s>(sql: &'s str, marker: &str) -> &'s str {
        let start = sql.find(marker).unwrap_or_else(|| panic!("{marker} in {sql}")) + marker.len();
        let end = sql[start..].find("::lquery").unwrap_or_else(|| panic!("an lquery after {marker} in {sql}"));
        &sql[start..start + end]
    }

    /// Whether `sql` compares `value_kind` anywhere, which a projected column
    /// never does.
    fn compares_the_value_kind(sql: &str) -> bool {
        [r#""value_kind" ="#, r#""value_kind" <>"#, r#""value_kind" IN"#, "value_kind = "]
            .iter()
            .any(|comparison| sql.contains(comparison))
    }

    #[test]
    fn a_scope_query_over_a_type_walk_reads_the_recorded_scope_paths() {
        let stage1 = recorded_stage1(&scope_query("/city/moved1"));
        assert_eq!(stage1.planning, StatementPlanning::GenericGatedBranches);
        let sql = stage1.select.to_string(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        assert!(
            candidates.starts_with(concat!(
                r#"SELECT "scope_history"."entity_key" FROM "scope_history" WHERE "scope_history"."value_kind" = 1 AND "#,
                r#"("scope_history"."scope_path" ~ ("#
            )),
            "{candidates}"
        );
        assert_eq!(lquery_after(candidates, CANDIDATE_PATH), "'city.moved1'");
        let (_, values) = stage1.select.build(PostgresQueryBuilder);
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
    }

    #[test]
    fn the_scope_candidates_state_the_partial_predicate_the_index_holds() {
        let sql = type_page("/city/moved1");
        assert!(PageBranches::of(&sql).candidates.contains(r#""scope_history"."value_kind" = 1"#), "{sql}");
    }

    #[test]
    fn the_per_entity_scope_probe_never_states_the_partial_predicate() {
        let sql = type_page("/city/moved1");
        let branches = PageBranches::of(&sql);
        for residual in [branches.short(), branches.indexed, branches.walk] {
            assert!(residual.contains(r#"FROM "scope_history" AS "rsh""#), "{residual}");
            assert!(!compares_the_value_kind(residual), "{residual}");
        }
        let leaf = build_filter_leaf_truths(
            &HistoryBranch::live_recorded(),
            &scope_query("/city/moved1"),
            &ids(&["urn:ngsi-ld:Sensor:000001"]),
            &sensor_names(),
        )
        .expect("renders")
        .to_string(PostgresQueryBuilder);
        assert!(leaf.contains(r#"FROM "scope_history" AS "rsh""#), "{leaf}");
        assert!(!compares_the_value_kind(&leaf), "{leaf}");
    }

    #[test]
    fn the_scope_path_test_is_the_one_the_probe_renders() {
        for raw in ["/city/moved1", "/city/+", "/city/#"] {
            let sql = type_page(raw);
            let branches = PageBranches::of(&sql);
            assert_eq!(
                lquery_after(branches.candidates, CANDIDATE_PATH),
                lquery_after(branches.indexed, PROBE_PATH),
                "{raw}"
            );
        }
    }

    #[test]
    fn a_conjunctive_scope_query_reads_rows_matching_any_of_its_paths() {
        let sql = type_page("(/A;/B),/C");
        let branches = PageBranches::of(&sql);
        assert!(
            branches.candidates.ends_with(concat!(
                r#" AND (("scope_history"."scope_path" ~ ('A'::lquery)) OR ("scope_history"."scope_path" ~ ('B'::lquery))"#,
                r#" OR ("scope_history"."scope_path" ~ ('C'::lquery)))"#
            )),
            "{}",
            branches.candidates
        );
        assert_eq!(branches.indexed.matches(r#") AS "sh" WHERE"#).count(), 3, "{}", branches.indexed);
        let a = branches.indexed.find("('A'::lquery)").expect("the probe of /A");
        let b = branches.indexed.find("('B'::lquery)").expect("the probe of /B");
        let c = branches.indexed.find("('C'::lquery)").expect("the probe of /C");
        assert!(branches.indexed[a..b].contains("') AND EXISTS(SELECT 1 FROM"), "{}", branches.indexed);
        assert!(branches.indexed[b..c].contains("')) OR EXISTS(SELECT 1 FROM"), "{}", branches.indexed);
    }

    #[test]
    fn the_window_and_the_validity_overlap_stay_in_the_probe() {
        let mut compiled = scope_query("/city/moved1");
        compiled.window.timeproperty = TimeProp::Modified;
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        let sql = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        for axis in ["recorded_at", "observed_at", "created_at", "modified_at", "deleted_at"] {
            assert!(!branches.candidates.contains(axis), "{axis}: {}", branches.candidates);
        }
        assert!(branches.indexed.contains(r#""sh"."modified_at" IS NOT NULL"#), "{}", branches.indexed);
        assert!(branches.indexed.contains(r#"AS "sh_next""#), "{}", branches.indexed);
    }

    #[test]
    fn a_scope_page_falls_to_the_walk_past_ten_thousand_rows() {
        let sql = type_page("/city/moved1");
        let branches = PageBranches::of(&sql);
        assert!(branches.walk.contains(r#"(SELECT COUNT(*) FROM "candidates") > 10000"#), "{}", branches.walk);
        assert!(
            branches.indexed.contains(r#"(SELECT COUNT(*) FROM "candidates") <= 10000"#),
            "{}",
            branches.indexed
        );
        assert!(branches.short().contains("LIMIT 200 OFFSET 0"), "{}", branches.short());
    }

    #[test]
    fn a_queued_branch_admits_every_queued_entity_for_its_scope() {
        let sql = stage1_on(&HistoryBranch::live(), &scope_query("/city/moved1"), &PageWindow::offset(20, 0))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches.candidates.starts_with(concat!(
                r#"SELECT "entities"."id" FROM "scope_history""#,
                r#" JOIN "entities" ON "entities"."entity_key" = "scope_history"."entity_key""#,
                r#" WHERE "scope_history"."value_kind" = 1 AND "#
            )),
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
    fn a_snapshot_reads_its_own_scope_path_index() {
        let sql = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &scope_query("/city/moved1"),
            &PageWindow::offset(20, 0),
        )
        .select
        .to_string(PostgresQueryBuilder);
        assert!(PageBranches::of(&sql).candidates.contains(r#"FROM "snap_7"."scope_history" WHERE"#), "{sql}");
    }

    /// Clause 5.7.4.4 applies the Scope query of a split query only to the
    /// aggregated Entities, so a contributed Entity is a candidate whatever its
    /// local Scope rows hold, and the residual still exempts it.
    #[test]
    fn a_contributed_entity_is_a_candidate_whatever_its_scope() {
        let mut compiled = scope_query("/city/moved1");
        compiled.contributed = ContributedEntities::new(ids(&["urn:ngsi-ld:Sensor:000009"]), ContributedRendering::Local);
        let recorded = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&recorded);
        assert!(
            branches.candidates.ends_with(concat!(
                r#" UNION ALL (SELECT "entities"."entity_key" FROM "entities""#,
                r#" WHERE "entities"."id" = 'urn:ngsi-ld:Sensor:000009' AND "entities"."evolution_created_at" IS NOT NULL)"#
            )),
            "{}",
            branches.candidates
        );
        assert!(
            branches.indexed.contains(r#" OR "e"."id" = 'urn:ngsi-ld:Sensor:000009'"#),
            "{}",
            branches.indexed
        );
        let queued = stage1_on(&HistoryBranch::live(), &compiled, &PageWindow::offset(20, 0))
            .select
            .to_string(PostgresQueryBuilder);
        assert!(
            PageBranches::of(&queued).candidates.contains(concat!(
                r#" UNION ALL (SELECT "entities"."id" FROM "entities""#,
                r#" WHERE "entities"."id" = 'urn:ngsi-ld:Sensor:000009' AND "entities"."evolution_created_at" IS NOT NULL)"#
            )),
            "{queued}"
        );
    }

    #[test]
    fn a_counted_scope_query_counts_under_the_count_bound() {
        let stage1 = recorded_stage1(&scope_query("/city/moved1"));
        assert_eq!(stage1.count_planning, StatementPlanning::BoundValues);
        let (sql, values) = stage1.count.expect("count requested").build(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"WITH "candidates" AS  MATERIALIZED (SELECT "scope_history"."entity_key" FROM"#),
            "{sql}"
        );
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_000))), "{:?}", values.0);
        let parts = CountParts::of(&sql);
        assert!(
            parts.indexed.contains(r#""entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{}",
            parts.indexed
        );
        assert_eq!(parts.walked.matches("candidates").count(), 2, "the two gates alone: {}", parts.walked);
        assert!(parts.filtered.contains(r#"FROM "scope_history" AS "rsh""#), "{}", parts.filtered);
        assert!(!parts.indexed.contains("scope_history") && !parts.walked.contains("scope_history"), "{sql}");
        assert!(!sql.contains("CASE WHEN"), "{sql}");
    }
}
