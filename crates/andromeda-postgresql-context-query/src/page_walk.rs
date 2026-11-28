//! The walks of a gated page: the selection read in page order, each Entity
//! it yields tested and probed on its own.
//!
//! The short walk visits the first Entities of the selection, the continued
//! walk the next ones when the short walk's matches project the page full
//! within them, and the full walk every Entity. The continued walk skips what
//! the short walk visited through the `OFFSET` of its selection, which reads
//! the skipped Entities without probing them, and hands on the short walk's
//! matches with its own.
//!
//! Every walk reads the selection as a subquery in page order and probes each
//! Entity it yields in a lateral subquery behind `OFFSET 0`, which the
//! planner can neither merge into the walk nor turn into a join. Probed as a
//! filter of the walk, a term the planner prices as rare becomes a hash of
//! every match against the whole selection, which reads every Entity however
//! soon the page fills: among 500 000 Entities a term 50 000 of them hold,
//! spread through the selection, cuts its page in 84 ms that way and in 3 ms
//! walked.
//!
//! The full walk, fenced by `OFFSET 0` alone, is planned to yield every row,
//! so whatever it reads beside the selection is planned as a join of every
//! Entity, sorted by identifier before the first probe. A walk therefore
//! reads its selection by the selectors alone and, when they or its sort keys
//! read the search row, looks it up for each Entity it yields in a lateral
//! subquery behind `OFFSET 0` keyed on the Entity's `entity_key`. Joined
//! plainly, the search row makes a merge join of
//! every Entity with every search row, 343 ms among 500 000 Entities with a
//! 22 MB external sort. A walk that fills its page from matches one Entity in
//! ten holds takes 2.0 ms looked up per Entity, against 290 ms joined plainly.
//!
//! Every other filter (`attrs`, the projected members, `scopeQ`) is tested on
//! each Entity the walk yields, through a search row it looks up for itself,
//! in one more such subquery outside the walk's `LIMIT`, so the short walk
//! visits its length
//! of the selection whatever the filters keep. As a filter of the walk, the
//! standing instance `attrs` reads becomes a semi-join over every carrier of
//! the Attribute, which sorts every Entity by identifier in 946 ms: a keyed
//! page beside `attrs` takes 757 to 817 ms that way and 45 ms tested per
//! Entity, as without `attrs`. Tested inside the `LIMIT`, a filter few
//! Entities pass makes the short walk read the whole selection before it
//! falls short. When no read answers, such a filter still makes the full walk
//! visit the whole selection, so a gated page also reads it from its own
//! index ([`filter_reads`](crate::filter_reads)).

use crate::{
    entity::{EntityConditions, all_of},
    entity_page::ProjectedPage,
    gated_page::{PageSource, cut},
};
use andromeda_ngsi_ld_query::{
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    one_time_gate::{ShortWalkMatches, count_of, wrapped},
    sort_key::KeyOrder,
};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, SimpleExpr, UnionType};

/// The name the short walk reads the Entities it probes under.
#[derive(Iden, Clone, Copy)]
#[iden = "walked"]
pub(crate) struct Walked;

/// The name of the probe a walk joins to each Entity it visits, one row when
/// the Entity passes and none otherwise.
#[derive(Iden, Clone, Copy)]
#[iden = "kept"]
struct Kept;

/// The name of the filters a walk tests each Entity it yields by, one row
/// when the Entity passes and none otherwise.
#[derive(Iden, Clone, Copy)]
#[iden = "filtered"]
struct Filtered;

/// The rows the short walk's probe reads: `entity_key` and `id` from the
/// walked Entity, in the branch of `rows` and standing at their instant.
pub(crate) fn walked_rows(rows: &EntityRowRefs) -> EntityRowRefs {
    EntityRowRefs::unaliased(rows.schema().cloned()).keyed_by(Walked).standing_at(rows.standing())
}

/// The walks of a gated page: the selection in page order, each Entity it
/// yields probed for the filter.
pub(crate) struct PageWalk<'a> {
    /// What the walks read: the selection's relations, or fewer when the
    /// probe reads the rest.
    pub(crate) source: PageSource<'a>,
    /// The filters the walks keep every Entity they visit by: the selection
    /// they read, and the rest, over [`walked_rows`], tested on each Entity it
    /// yields, so the short walk visits `short` Entities of the selection at
    /// most.
    pub(crate) walked: EntityConditions,
    /// The filter each visited Entity is probed for, over [`walked_rows`].
    pub(crate) probe: Condition,
    /// How many Entities the short walk visits.
    pub(crate) short: u32,
    /// How many Entities past the short walk the continued walk visits, when
    /// the short walk's matches project the page full within them; `None`
    /// when the page never walks on.
    pub(crate) continued: Option<u32>,
}

/// The short walk: the first `walk.short` Entities of the selection in page
/// order, from where `window` starts, that the filters and the probe keep, at
/// most `need` of them, and the order its rows come back in.
///
/// The filters and the probe test the Entities the walk yields rather than
/// the walk itself, so the walk visits `walk.short` Entities at most whatever
/// they keep. A walk shorter than `need` cannot fill the page, so it visits
/// none.
pub(crate) fn short_walk(walk: &PageWalk<'_>, window: &PageWindow, need: u64) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let (mut matches, read_order) = walked_in_order(
        &walk.source,
        walk.walked.clone(),
        walk.probe.clone(),
        Vec::new(),
        window,
        WalkSpan::First(short_length(walk, need)),
    )?;
    matches.select.limit(need);
    Ok((matches, read_order))
}

/// The continued walk over the short walk's matches ([`ShortWalkMatches`]),
/// which holds those matches and then the Entities among the next
/// `walk.continued` of the selection in page order that the filters and the
/// probe keep, at most `need` together, ordered as `order` orders the page.
/// `None` when the page never walks on, its short walk visits no Entity, or
/// no number of the short walk's matches short of `need` projects the page
/// full.
///
/// The walk on runs only when the short walk fell short of `need` with
/// matches enough to project the page full within the continued walk
/// ([`projecting_matches`]). It needs the matches the short walk lacks, which
/// the `LIMIT` of the whole stops it at when PostgreSQL merges the two walks
/// in page order, and never more than the gate leaves the short walk
/// lacking.
pub(crate) fn continued_walk(walk: &PageWalk<'_>, window: &PageWindow, need: u64, order: &[(Alias, KeyOrder)]) -> Result<Option<SelectStatement>, RenderError> {
    let Some(continued) = walk.continued else {
        return Ok(None);
    };
    let short = short_length(walk, need);
    let Some(least) = projecting_matches(short, continued, need) else {
        return Ok(None);
    };
    let gates = vec![count_of(ShortWalkMatches).lt(need), count_of(ShortWalkMatches).gte(least)];
    let span = WalkSpan::Past {
        skipped: short,
        visited: continued,
    };
    let (on, _) = walked_in_order(&walk.source, walk.walked.clone(), walk.probe.clone(), gates, window, span)?;
    let mut on = on.select;
    on.limit(need - least);
    let mut walks = Query::select();
    walks.column(Asterisk).from(ShortWalkMatches).union(UnionType::All, wrapped(on, "walked_on"));
    let mut matches = wrapped(walks.take(), "walks");
    for (column, key_order) in order {
        key_order.order_by(&mut matches, Expr::col(column.clone()));
    }
    matches.limit(need);
    Ok(Some(matches))
}

/// The fewest matches a short walk of `short` Entities keeps that project a
/// page needing `need` full within the `continued` Entities past it, `None`
/// when no number of them short of `need` does.
///
/// Matches found at the short walk's rate fill the page after `need * short /
/// matches` Entities, so the projection stays within both walks from `need *
/// short / (short + continued)` matches on, rounded up.
fn projecting_matches(short: u32, continued: u32, need: u64) -> Option<u64> {
    let visited = u64::from(short) + u64::from(continued);
    if short == 0 || visited == 0 {
        return None;
    }
    let least = (need * u64::from(short)).div_ceil(visited);
    (least < need).then_some(least)
}

/// How many Entities the short walk of `walk` visits for a page needing
/// `need`, none when it is shorter than `need`, since it cannot fill the page.
fn short_length(walk: &PageWalk<'_>, need: u64) -> u32 {
    if need > u64::from(walk.short) { 0 } else { walk.short }
}

/// The full walk: every Entity of the selection in page order the filters
/// and the probe keep, cut to `window`, behind the gates `fell_short` of
/// every earlier step.
pub(crate) fn full_walk(walk: PageWalk<'_>, window: &PageWindow, fell_short: Vec<SimpleExpr>) -> Result<SelectStatement, RenderError> {
    let (matches, _) = walked_in_order(&walk.source, walk.walked, walk.probe, fell_short, window, WalkSpan::Every)?;
    let mut matches = matches.select;
    cut(&mut matches, window);
    Ok(matches)
}

/// Which Entities of the selection a walk visits, in page order from where
/// the window starts.
#[derive(Clone, Copy)]
enum WalkSpan {
    /// The first this many.
    First(u32),
    /// `visited` of them past the first `skipped`.
    Past {
        /// How many the walk passes over unprobed.
        skipped: u32,
        /// How many it visits after them.
        visited: u32,
    },
    /// Every one.
    Every,
}

/// The selection `walked` reads, in page order from where `window` starts,
/// the Entities of it `span` names, each it yields kept when the rest of
/// `walked` and `probe` hold over [`walked_rows`], ordered as the page is,
/// behind `gates`.
///
/// The selection is a subquery behind its `LIMIT`, or behind `OFFSET 0`
/// without one, and the filters and the probe lateral subqueries behind
/// `OFFSET 0`, which keeps the planner from merging any into another: it
/// visits the Entities in page order, tests each in turn, and stops once the
/// statement has its rows. The selection reads the walked relations
/// ([`EntityRelations::walked`](crate::entity_relations::EntityRelations::walked))
/// by its selectors, so a search row it reads is looked up for each Entity it
/// yields.
fn walked_in_order(
    source: &PageSource<'_>,
    walked: EntityConditions,
    probe: Condition,
    gates: Vec<SimpleExpr>,
    window: &PageWindow,
    span: WalkSpan,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let start = match &window.start {
        PageStart::Offset(_) => PageStart::Offset(0),
        PageStart::Cursor(cursor) => PageStart::Cursor(cursor.clone()),
    };
    let limit = match span {
        WalkSpan::First(length) | WalkSpan::Past { visited: length, .. } => length,
        WalkSpan::Every => 0,
    };
    let EntityConditions { selection: selectors, tested } = walked;
    let (selection, read_order) = source.page_through(source.relations.walked(source.query), all_of(selectors), &PageWindow { limit, start })?;
    let ProjectedPage { select: mut selection, order } = selection;
    match span {
        WalkSpan::First(_) => {}
        WalkSpan::Past { skipped, .. } => {
            selection.offset(u64::from(skipped));
        }
        WalkSpan::Every => {
            selection.reset_limit().offset(0);
        }
    }
    let mut matches = Query::select();
    matches.column((Walked, Asterisk)).from_subquery(selection, Walked);
    if !tested.is_empty() {
        matches.join_lateral(
            JoinType::InnerJoin,
            searched(&walked_rows(source.rows), all_of(tested)),
            Filtered,
            Condition::all(),
        );
    }
    matches.join_lateral(JoinType::InnerJoin, fenced(probe), Kept, Condition::all());
    for gate in gates {
        matches.and_where(gate);
    }
    for (column, key_order) in &order {
        key_order.order_by(&mut matches, Expr::col((Walked, column.clone())));
    }
    Ok((ProjectedPage { select: matches.take(), order }, read_order))
}

/// `SELECT 1` when `condition` holds, behind `OFFSET 0`, which the planner can
/// neither merge into the statement it is joined to nor turn into a join.
fn fenced(condition: Condition) -> SelectStatement {
    let mut fence = Query::select();
    fence.expr(int_literal(1)).cond_where(condition).offset(0);
    fence.take()
}

/// `SELECT 1` from the search row of the Entity `walked` names, looked up by
/// its key, when `condition` holds over it, behind `OFFSET 0` as [`fenced`].
fn searched(walked: &EntityRowRefs, condition: Condition) -> SelectStatement {
    let mut fence = Query::select();
    fence
        .expr(int_literal(1))
        .from(walked.table(EntitySearch::Table))
        .cond_where(
            Condition::all()
                .add(walked.search_column(EntitySearch::EntityKey).eq(walked.entity_key_column()))
                .add(condition),
        )
        .offset(0);
    fence.take()
}

#[cfg(test)]
mod tests {
    use crate::page_walk::projecting_matches;
    use pretty_assertions::assert_eq;

    /// Matches at the short walk's rate fill the page within both walks from
    /// `need * 1 000 / 5 000` of them, rounded up, and 101 / 5 is 20.2.
    #[test]
    fn the_short_walk_projects_the_page_full_from_the_rounded_up_rate() {
        assert_eq!(projecting_matches(1000, 4000, 101), Some(21));
        assert_eq!(projecting_matches(1000, 4000, 21), Some(5));
    }

    /// A page needing one match is filled by the short walk's first, and a
    /// short walk visiting no Entity measures nothing.
    #[test]
    fn no_count_short_of_the_need_projects_a_page_of_one_or_a_walk_of_none() {
        assert_eq!(projecting_matches(1000, 4000, 1), None);
        assert_eq!(projecting_matches(1000, 4000, 0), None);
        assert_eq!(projecting_matches(0, 4000, 101), None);
    }
}
