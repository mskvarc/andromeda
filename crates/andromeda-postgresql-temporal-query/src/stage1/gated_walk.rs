//! The stage 1 page a candidate step cuts.
//!
//! Left the choice, the planner either walks the Evolutions in identifier
//! order and probes each one, or reads the step's index and sorts what it
//! yields, and it cannot price the term the index answers. The statement
//! therefore tries three ways, and a gate on what the previous one found
//! switches each later one off:
//!
//! 1. the short walk: the first Evolutions of the walk in identifier order,
//!    from where the page starts, each probed for every filter, at most as
//!    many kept as the page needs. A term most Evolutions pass fills the page
//!    here. A page needing more matches than the short walk visits
//!    Evolutions cannot fill there, so its statement holds no short walk and
//!    its candidates decide alone;
//! 2. the candidates: read up to one past the step's bound, and the page cut
//!    from the walk narrowed to them when they number at most the bound, every
//!    filter the step does not answer tested on what it yields;
//! 3. the full walk: every filter tested on every Evolution, when more
//!    candidates qualify, or, for a step whose read has a floor of its own,
//!    when the short walk's matches project the page full within a bounded
//!    walk, so the candidates are never read.
//!
//! Each way is one branch of a `UNION ALL` behind a one-time gate, and every
//! branch orders and cuts the same window, so whichever runs returns the page
//! the plain statement returns.

use crate::{
    alias::EntityIdColumn,
    stage1::{
        candidate_step::CandidateStep,
        entity_conditions::{PatternTest, build_entity_conditions_for_temporal},
        evolution_walk::{Among, evolution_rows},
        selected_ids,
        window_match::temporal_match_subquery,
    },
};
use andromeda_ngsi_ld_query::{
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    entity_ordering::sql_order,
    error::RenderError,
    history_source::{BesideComparison, HistoryBranch},
    one_time_gate::{Candidates, ShortWalkMatches, bounded, count_of, materialized, within, wrapped},
    page_window::apply_identifier_window,
};
use andromeda_postgresql_schema::current::{E, Entities};
use sea_query::{Asterisk, Condition, Expr, ExprTrait, IntoIden, Query, SelectStatement, SimpleExpr, TableRef, UnionType, WithClause};

/// The page of stage 1 that `step` cuts, and the order its identifiers come
/// back in.
///
/// # Errors
///
/// Fails as rendering the plain page fails.
pub(super) fn gated_walk(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    window: &PageWindow,
    names: &QueryNames,
    step: &CandidateStep<'_>,
) -> Result<(SelectStatement, ReadOrder), RenderError> {
    let need = match window.start {
        PageStart::Offset(offset) => u64::from(offset) + u64::from(window.limit),
        PageStart::Cursor(_) => u64::from(window.limit),
    };
    let window_match = || temporal_match_subquery(branch, compiled, names, BesideComparison::Identifier);
    let walked = evolution_rows(branch, compiled, names, Among::Every);
    let whole = build_entity_conditions_for_temporal(branch, compiled, names, walked.type_test, PatternTest::Tested)?;
    let narrowed = evolution_rows(branch, compiled, names, Among::Candidates);
    let residual = build_entity_conditions_for_temporal(branch, compiled, names, narrowed.type_test, step.pattern_test())?;

    // A walk shorter than the page needs cannot fill it, so it visits none.
    let visited = if need > u64::from(step.short_walk()) { 0 } else { step.short_walk() };
    let length = ShortWalkLength { visited, need };
    let short = (visited > 0).then(|| short_walk(walked.paged.clone(), whole.clone(), window_match(), window, length));
    let gates = Gates::of(step, length);

    let mut indexed = selected_ids(narrowed.paged, gates.indexed.into_iter().fold(residual, Condition::add), window_match());
    let read_order = apply_identifier_window(&mut indexed, Expr::col((E, Entities::Id)), window);
    let mut walk = selected_ids(walked.paged, gates.walk.into_iter().fold(whole, Condition::add), window_match());
    apply_identifier_window(&mut walk, Expr::col((E, Entities::Id)), window);

    let mut ctes = WithClause::new();
    let mut union = match short {
        Some(short) => {
            ctes.cte(materialized(short, ShortWalkMatches));
            let mut union = wrapped(answered(window, read_order, need), "answered");
            union.union(UnionType::All, wrapped(indexed, "indexed"));
            union
        }
        None => wrapped(indexed, "indexed"),
    };
    ctes.cte(materialized(bounded(step.candidates(branch), step.bound()), Candidates));
    union.union(UnionType::All, wrapped(walk, "walk")).with_cte(ctes);
    Ok((union.take(), read_order))
}

/// The branch that returns the page from the short walk's matches, when
/// they number at least the `need` the page has from where the walk starts.
fn answered(window: &PageWindow, read_order: ReadOrder, need: u64) -> SelectStatement {
    let mut answered = Query::select();
    answered
        .column(Asterisk)
        .from(ShortWalkMatches)
        .and_where(count_of(ShortWalkMatches).gte(need))
        .order_by(EntityIdColumn, sql_order(read_order.into()));
    match window.start {
        PageStart::Offset(offset) => {
            answered.offset(u64::from(offset));
        }
        PageStart::Cursor(_) => {}
    }
    answered.limit(u64::from(window.limit));
    answered
}

/// How far a short walk goes.
#[derive(Debug, Clone, Copy)]
struct ShortWalkLength {
    /// How many Evolutions it visits: none when the page needs more matches
    /// than the step's short walk visits Evolutions, and the statement then
    /// holds no short walk.
    visited: u32,
    /// How many matches the page needs from where the walk starts.
    need: u64,
}

/// The one-time gates of the candidate branch and of the full walk, each a
/// list of conditions added to the branch's own.
#[derive(Debug)]
struct Gates {
    /// The candidate branch runs when every one holds.
    indexed: Vec<SimpleExpr>,
    /// The full walk runs when every one holds.
    walk: Vec<SimpleExpr>,
}

impl Gates {
    /// The gates of `step` behind a short walk of `length`.
    ///
    /// Without a short walk, the candidates decide alone: at most the bound
    /// and the candidate branch runs, past it the full walk. Without a dense
    /// walk, they decide the same way once the short walk falls short. With
    /// one, a short walk whose matches project the page full within the
    /// dense walk sends the page to the full walk unread, which
    /// stops at the page's last match. A `CASE` fixes the order: PostgreSQL
    /// orders a branch's one-time conditions by their cost, and only this
    /// order leaves the candidates unread.
    fn of(step: &CandidateStep<'_>, length: ShortWalkLength) -> Gates {
        let bound = step.bound();
        if length.visited == 0 {
            return Gates {
                indexed: vec![within(Candidates, bound)],
                walk: vec![count_of(Candidates).gt(bound)],
            };
        }
        let Some(least) = dense_matches(step.dense_walk(), length) else {
            let walked_short = count_of(ShortWalkMatches).lt(length.need);
            return Gates {
                indexed: vec![walked_short.clone(), within(Candidates, bound)],
                walk: vec![walked_short, count_of(Candidates).gt(bound)],
            };
        };
        let filled = || count_of(ShortWalkMatches).gte(length.need);
        let dense = || count_of(ShortWalkMatches).gte(least);
        Gates {
            indexed: vec![Expr::case(filled(), false).case(dense(), false).finally(within(Candidates, bound)).into()],
            walk: vec![Expr::case(filled(), false).case(dense(), true).finally(count_of(Candidates).gt(bound)).into()],
        }
    }
}

/// The fewest short-walk matches that project a page needing `length.need`
/// full within `dense_walk` Evolutions, `None` when no number of them can tell
/// before the page fills.
///
/// Matches found at the short walk's rate fill the page after `need *
/// visited / matches` Evolutions, so the projection stays within the dense
/// walk from `need * visited / dense_walk` matches on, rounded up.
fn dense_matches(dense_walk: Option<u32>, length: ShortWalkLength) -> Option<u64> {
    let dense_walk = u64::from(dense_walk?);
    if length.visited == 0 {
        return None;
    }
    let least = (length.need * u64::from(length.visited)).div_ceil(dense_walk);
    (least < length.need).then_some(least)
}

/// The short walk: the first `length.visited` Evolutions of `evolutions` from
/// where `window` starts, in the window's order, that `conditions` and the
/// window match keep, at most `length.need` of them.
///
/// The filters test what the walk yields rather than the walk itself, so the
/// walk visits that many Evolutions at most whatever they match.
fn short_walk(evolutions: TableRef, conditions: Condition, window_match: SelectStatement, window: &PageWindow, length: ShortWalkLength) -> SelectStatement {
    let start = match &window.start {
        PageStart::Offset(_) => PageStart::Offset(0),
        PageStart::Cursor(cursor) => PageStart::Cursor(cursor.clone()),
    };
    let mut walk = Query::select();
    walk.column(Asterisk).from(evolutions);
    let read_order = apply_identifier_window(&mut walk, Expr::col((E, Entities::Id)), &PageWindow { limit: length.visited, start });
    let mut matches = selected_ids(TableRef::SubQuery(Box::new(walk.take()), E.into_iden()), conditions, window_match);
    matches.order_by((E, Entities::Id), sql_order(read_order.into())).limit(length.need);
    matches
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        gated_walk::{ShortWalkLength, dense_matches},
        query_fixture::{PageBranches, id_pattern, recorded_stage1, stage1_on},
    };
    use andromeda_ngsi_ld_query::page_window::PageWindow;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// The rendered page of `idPattern=.*:0123.*` over the catalogue.
    fn catalogue_page() -> String {
        recorded_stage1(&id_pattern(None, ".*:0123.*")).select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_candidate_branch_keeps_the_window_match_and_drops_the_pattern() {
        let sql = catalogue_page();
        let indexed = PageBranches::of(&sql).indexed;
        assert!(indexed.contains(r#""e"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#), "{indexed}");
        assert!(indexed.contains("EXISTS(SELECT 1 FROM"), "{indexed}");
        assert!(!indexed.contains(r#""e"."id" ~"#) && !indexed.contains(r#""e"."id") COLLATE"#), "{indexed}");
    }

    #[test]
    fn the_candidate_branch_orders_by_identifier_before_its_residual() {
        let sql = catalogue_page();
        let indexed = PageBranches::of(&sql).indexed;
        assert!(
            indexed.contains(concat!(
                r#"FROM (SELECT * FROM (SELECT * FROM "entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e" WHERE "e"."entity_key" IN (SELECT "entity_key" FROM "candidates")"#,
                r#" ORDER BY "id" ASC OFFSET 0) AS "e" WHERE"#
            )),
            "{indexed}"
        );
    }

    #[test]
    fn the_full_walk_keeps_the_pattern_past_the_bound() {
        let sql = catalogue_page();
        let walk = PageBranches::of(&sql).walk;
        assert!(walk.contains(r#"("e"."id") COLLATE "default") ~ '.*:0123.*'"#), "{walk}");
        assert!(walk.contains(r#"(SELECT COUNT(*) FROM "candidates") > 1000"#), "{walk}");
    }

    #[test]
    fn the_short_walk_tests_the_first_five_thousand_evolutions() {
        let sql = catalogue_page();
        let branches = PageBranches::of(&sql);
        let short = branches.short();
        assert!(
            short.contains(r#"FROM (SELECT * FROM (SELECT * FROM "entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e" ORDER BY "e"."id" ASC LIMIT 5000 OFFSET 0) AS "e" WHERE"#),
            "{short}"
        );
        assert!(short.contains(r#"("e"."id") COLLATE "default") ~ '.*:0123.*'"#), "{short}");
    }

    /// 21 matches behind a 200-Evolution short walk fill within a
    /// 1,500-Evolution walk at the rate of 21 * 200 / 1,500 = 2.8 matches,
    /// so 3 project the page full.
    #[test]
    fn the_short_walk_projects_the_page_full_from_the_rounded_up_rate() {
        assert_eq!(dense_matches(Some(1_500), ShortWalkLength { visited: 200, need: 21 }), Some(3));
        assert_eq!(dense_matches(Some(1_500), ShortWalkLength { visited: 200, need: 51 }), Some(7));
    }

    #[test]
    fn no_projection_stands_without_a_dense_walk_or_a_short_walk() {
        assert_eq!(dense_matches(None, ShortWalkLength { visited: 200, need: 21 }), None);
        assert_eq!(dense_matches(Some(1_500), ShortWalkLength { visited: 0, need: 270 }), None);
    }

    /// A page of one is filled by its first match, so no fewer matches can
    /// project it full.
    #[test]
    fn a_projection_needing_the_whole_page_is_no_projection() {
        assert_eq!(dense_matches(Some(1_500), ShortWalkLength { visited: 200, need: 1 }), None);
        assert_eq!(dense_matches(Some(100), ShortWalkLength { visited: 200, need: 20 }), None);
    }

    /// A Gather under a gated-off branch still launches its workers, so a
    /// page whose candidates are an index read runs serially.
    #[test]
    fn an_index_read_page_runs_serially() {
        let stage1 = recorded_stage1(&id_pattern(None, ".*:0123.*"));
        assert_eq!(stage1.planning, StatementPlanning::GenericGatedBranches);
    }

    #[test]
    fn a_step_without_a_dense_walk_keeps_its_plain_gates() {
        let sql = catalogue_page();
        let walk = PageBranches::of(&sql).walk;
        assert!(!walk.contains("CASE"), "{walk}");
        assert!(
            walk.contains(r#"(SELECT COUNT(*) FROM "short_walk") < 20 AND (SELECT COUNT(*) FROM "candidates") > 1000"#),
            "{walk}"
        );
    }

    #[test]
    fn a_page_beyond_the_short_walk_holds_no_short_walk() {
        let sql = stage1_on(&HistoryBranch::live_recorded(), &id_pattern(None, ".*:0123.*"), &PageWindow::offset(4990, 20))
            .select
            .to_string(PostgresQueryBuilder);
        assert_eq!(PageBranches::of(&sql).short_walk, None, "{sql}");
        assert!(sql.starts_with(r#"WITH "candidates" AS  MATERIALIZED ("#), "{sql}");
        assert!(!sql.contains("short_walk"), "{sql}");
    }

    #[test]
    fn a_page_without_a_short_walk_keeps_one_candidate_branch_and_one_walk() {
        let sql = stage1_on(&HistoryBranch::live_recorded(), &id_pattern(None, ".*:0123.*"), &PageWindow::offset(4990, 20))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert_eq!(sql.matches("UNION ALL").count(), 1, "{sql}");
        assert!(
            branches.indexed.contains(r#"(SELECT COUNT(*) FROM "candidates") <= 1000 AND"#),
            "{}",
            branches.indexed
        );
        assert!(branches.walk.contains(r#"(SELECT COUNT(*) FROM "candidates") > 1000 AND"#), "{}", branches.walk);
        assert!(branches.walk.ends_with("LIMIT 4990 OFFSET 20) AS \"walk\")"), "{}", branches.walk);
    }
}
