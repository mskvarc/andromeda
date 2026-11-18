//! The walks of a gated page: the selection read in page order, each Entity
//! it yields tested and probed on its own.
//!
//! Both walks read the selection as a subquery in page order and probe each
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
//! falls short.

use crate::{
    entity::{EntityConditions, all_of},
    entity_page::ProjectedPage,
    gated_page::{PageSource, cut},
};
use andromeda_ngsi_ld_query::{
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{entity_row_refs::EntityRowRefs, error::RenderError};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Asterisk, Condition, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, SimpleExpr};

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
    let length = if need > u64::from(walk.short) { 0 } else { walk.short };
    let (mut matches, read_order) = walked_in_order(&walk.source, walk.walked.clone(), walk.probe.clone(), Vec::new(), window, Some(length))?;
    matches.select.limit(need);
    Ok((matches, read_order))
}

/// The full walk: every Entity of the selection in page order the filters
/// and the probe keep, cut to `window`, behind the gates `fell_short` of
/// every earlier step.
pub(crate) fn full_walk(walk: PageWalk<'_>, window: &PageWindow, fell_short: Vec<SimpleExpr>) -> Result<SelectStatement, RenderError> {
    let (matches, _) = walked_in_order(&walk.source, walk.walked, walk.probe, fell_short, window, None)?;
    let mut matches = matches.select;
    cut(&mut matches, window);
    Ok(matches)
}

/// The selection `walked` reads, in page order from where `window` starts,
/// `length` Entities of it or every one, each it yields kept when the rest of
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
    length: Option<u32>,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let start = match &window.start {
        PageStart::Offset(_) => PageStart::Offset(0),
        PageStart::Cursor(cursor) => PageStart::Cursor(cursor.clone()),
    };
    let EntityConditions { selection: selectors, tested } = walked;
    let (selection, read_order) = source.page_through(
        source.relations.walked(source.query),
        all_of(selectors),
        &PageWindow {
            limit: length.unwrap_or(0),
            start,
        },
    )?;
    let ProjectedPage { select: mut selection, order } = selection;
    match length {
        Some(_) => {}
        None => {
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
