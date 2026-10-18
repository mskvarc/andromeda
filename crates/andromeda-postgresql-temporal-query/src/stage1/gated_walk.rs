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
//!    here;
//! 2. the candidates: read up to one past the step's bound, and the page cut
//!    from the walk narrowed to them when they number at most the bound, every
//!    filter the step does not answer tested on what it yields;
//! 3. the full walk: every filter tested on every Evolution, when more
//!    candidates qualify.
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
use andromeda_postgresql_schema::{current::E, temporal::TemporalEntities};
use sea_query::{Asterisk, Condition, Expr, ExprTrait, IntoIden, Query, SelectStatement, TableRef, UnionType, WithClause};

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

    let short = short_walk(
        // clone: the full walk reads the same Evolutions
        walked.paged.clone(),
        // clone: the full walk tests the same filters
        whole.clone(),
        window_match(),
        window,
        ShortWalkLength {
            visited: step.short_walk(),
            need,
        },
    );
    let walked_short = count_of(ShortWalkMatches).lt(need);

    let mut indexed = selected_ids(
        narrowed.paged,
        residual
            .add(walked_short.clone()) // clone: the full walk is gated on the short walk too
            .add(within(Candidates, step.bound())),
        window_match(),
    );
    let read_order = apply_identifier_window(&mut indexed, Expr::col((E, TemporalEntities::Id)), window);
    let mut walk = selected_ids(walked.paged, whole.add(walked_short).add(count_of(Candidates).gt(step.bound())), window_match());
    apply_identifier_window(&mut walk, Expr::col((E, TemporalEntities::Id)), window);

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

    let mut union = wrapped(answered.take(), "answered");
    union
        .union(UnionType::All, wrapped(indexed, "indexed"))
        .union(UnionType::All, wrapped(walk, "walk"))
        .with_cte(
            WithClause::new()
                .cte(materialized(short, ShortWalkMatches))
                .cte(materialized(bounded(step.candidates(branch), step.bound()), Candidates))
                .to_owned(),
        );
    Ok((union.take(), read_order))
}

/// How far a short walk goes.
#[derive(Debug, Clone, Copy)]
struct ShortWalkLength {
    /// How many Evolutions it visits.
    visited: u32,
    /// How many matches the page needs from where the walk starts.
    need: u64,
}

/// The short walk: the first `length.visited` Evolutions of `evolutions` from
/// where `window` starts, in the window's order, that `conditions` and the
/// window match keep, at most `length.need` of them.
///
/// The filters test what the walk yields rather than the walk itself, so the
/// walk visits that many Evolutions at most whatever they match. A walk
/// shorter than the page needs cannot fill it, so it visits none.
fn short_walk(evolutions: TableRef, conditions: Condition, window_match: SelectStatement, window: &PageWindow, length: ShortWalkLength) -> SelectStatement {
    let start = match &window.start {
        PageStart::Offset(_) => PageStart::Offset(0),
        // clone: the walk starts where the window does
        PageStart::Cursor(cursor) => PageStart::Cursor(cursor.clone()),
    };
    let visited = if length.need > u64::from(length.visited) { 0 } else { length.visited };
    let mut walk = Query::select();
    walk.column(Asterisk).from(evolutions);
    let read_order = apply_identifier_window(&mut walk, Expr::col((E, TemporalEntities::Id)), &PageWindow { limit: visited, start });
    let mut matches = selected_ids(TableRef::SubQuery(Box::new(walk.take()), E.into_iden()), conditions, window_match);
    matches.order_by((E, TemporalEntities::Id), sql_order(read_order.into())).limit(length.need);
    matches
}

#[cfg(test)]
mod tests {
    use crate::stage1::query_fixture::{PageBranches, id_pattern, recorded_stage1, stage1_on};
    use andromeda_ngsi_ld_query::page_window::PageWindow;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use sea_query::PostgresQueryBuilder;

    /// The rendered page of `idPattern=.*:0123.*` over the catalogue.
    fn catalogue_page() -> String {
        recorded_stage1(&id_pattern(None, ".*:0123.*")).select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_candidate_branch_keeps_the_window_match_and_drops_the_pattern() {
        let sql = catalogue_page();
        let indexed = PageBranches::of(&sql).indexed;
        assert!(
            indexed.contains(r#""e"."temporal_entity_key" IN (SELECT "temporal_entity_key" FROM "candidates")"#),
            "{indexed}"
        );
        assert!(indexed.contains("EXISTS(SELECT 1 FROM"), "{indexed}");
        assert!(!indexed.contains(r#""e"."id" ~"#), "{indexed}");
    }

    #[test]
    fn the_candidate_branch_orders_by_identifier_before_its_residual() {
        let sql = catalogue_page();
        let indexed = PageBranches::of(&sql).indexed;
        assert!(
            indexed.contains(concat!(
                r#"FROM (SELECT * FROM "temporal_entities" AS "e" WHERE "e"."temporal_entity_key" IN (SELECT "temporal_entity_key" FROM "candidates")"#,
                r#" ORDER BY "id" ASC OFFSET 0) AS "e" WHERE"#
            )),
            "{indexed}"
        );
    }

    #[test]
    fn the_full_walk_keeps_the_pattern_past_the_bound() {
        let sql = catalogue_page();
        let walk = PageBranches::of(&sql).walk;
        assert!(walk.contains(r#""e"."id" ~ '.*:0123.*'"#), "{walk}");
        assert!(walk.contains(r#"(SELECT COUNT(*) FROM "candidates") > 1000"#), "{walk}");
    }

    #[test]
    fn the_short_walk_tests_the_first_five_thousand_evolutions() {
        let sql = catalogue_page();
        let short = PageBranches::of(&sql).short;
        assert!(
            short.contains(r#"FROM (SELECT * FROM "temporal_entities" AS "e" ORDER BY "e"."id" ASC LIMIT 5000 OFFSET 0) AS "e" WHERE"#),
            "{short}"
        );
        assert!(short.contains(r#""e"."id" ~ '.*:0123.*'"#), "{short}");
    }

    #[test]
    fn a_page_beyond_the_short_walk_never_walks_it_short() {
        let sql = stage1_on(&HistoryBranch::live_recorded(), &id_pattern(None, ".*:0123.*"), &PageWindow::offset(4990, 20))
            .select
            .to_string(PostgresQueryBuilder);
        let short = PageBranches::of(&sql).short;
        assert!(short.contains(r#"ORDER BY "e"."id" ASC LIMIT 0 OFFSET 0) AS "e""#), "{short}");
        assert!(short.ends_with("LIMIT 5010"), "the walk keeps what the page needs: {short}");
    }
}
