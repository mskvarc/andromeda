//! A page whose selection an index narrows by a term the planner cannot
//! price, cut in the order that stops soonest.
//!
//! Left the choice, the planner either walks the selection in page order and
//! filters each Entity, or reads the term's index and sorts what it yields,
//! and it misjudges which costs less by orders of magnitude when the matches
//! cluster in page order or its estimate of the term is off. The statement
//! therefore tries each way in turn, and a gate on what the earlier ones found
//! switches each later one off:
//!
//! 1. a short walk: the first Entities of the selection in page order, each
//!    probed for the filter. A term most Entities pass fills the page here.
//! 2. the index reads, one term after another: the candidates read up to one
//!    past a bound, and the page cut from the selection narrowed to them when
//!    they number at most the bound;
//! 3. the full walk: the selection in page order, each Entity probed for the
//!    filter, when every read found more candidates than its bound.
//!
//! Each step is one branch of a `UNION ALL` behind a
//! [`one_time_gate`](andromeda_postgresql_query::one_time_gate). Every branch cuts the same window
//! over the same order, so whichever runs returns the page the plain
//! statement would.
//!
//! Both walks read the selection as a subquery in page order and probe each
//! Entity it yields in a lateral subquery behind `OFFSET 0`, which the
//! planner can neither merge into the walk nor turn into a join. Probed as a
//! filter of the walk, a term the planner prices as rare becomes a hash of
//! every match against the whole selection, which reads every Entity however
//! soon the page fills: among 500 000 Entities a term 50 000 of them hold,
//! spread through the selection, cuts its page in 84 ms that way and in 3 ms
//! walked. An index branch narrows the selection to its candidates as an
//! array ([`narrowed_to`](andromeda_postgresql_query::one_time_gate::narrowed_to)),
//! looked up one by one.
//!
//! The full walk, fenced by `OFFSET 0` alone, is planned to yield every row,
//! so whatever it reads beside the selection is planned as a join of every
//! Entity, sorted by identifier before the first probe. A walk therefore
//! reads its selection by the selectors alone, looks the search row up for
//! each Entity it yields in a lateral subquery behind `OFFSET 0` keyed on the
//! Entity's `entity_key`, and tests every other filter (`attrs`, the
//! projected members, `scopeQ`) on each Entity in one more such subquery.
//! Joined plainly, the search row makes a merge join of every Entity with
//! every search row, 343 ms among 500 000 Entities with a 22 MB external
//! sort, and the standing instance `attrs` reads makes a semi-join over every
//! `attrs` row, 946 ms: a keyed page beside `attrs` took 757 to 817 ms, and
//! takes 45 ms tested per Entity, as without `attrs`. A walk that fills its
//! page from matches one Entity in ten holds takes 2.0 ms looked up per
//! Entity, against 290 ms joined plainly. The index branches keep the plain
//! join and filters, since they look each candidate up by its key.
//!
//! A counted page's index branch carries the total (clause 6.3.13) as
//! `COUNT(*) OVER ()`: it reads every candidate before it cuts the window,
//! so the count costs no second read of them. The walks carry no total, the
//! short walk having seen part of the selection and the full walk stopping
//! once the page is cut. Each branch can also name itself on its rows
//! ([`GatedStep`]), so the count run when the page carries no total knows
//! what the page found.

use crate::{
    entity::{EntityConditions, GeoFilter, QFilter, all_of, build_entity_conditions, entity_conditions, window_and_sort_keys},
    entity_page::{GATED_STEP_COLUMN, GatedStep, ProjectedPage, StepColumn, TOTAL_COUNT_COLUMN, projected_page},
    entity_relations::EntityRelations,
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    one_time_gate::{CandidateBound, Candidates, ShortWalkMatches, beyond, column_of, count_of, materialized, narrowed_to, within, wrapped},
    query_names::QueryNames,
};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{aggregate::count_over_all, literal::int_literal};
use sea_query::{Alias, Asterisk, Condition, DynIden, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, SimpleExpr, UnionType, Value, WithClause};

/// The name the short walk reads the Entities it probes under.
#[derive(Iden, Clone, Copy)]
#[iden = "walked"]
pub(crate) struct Walked;

/// The name of the probe a walk joins to each Entity it visits, one row when
/// the Entity passes and none otherwise.
#[derive(Iden, Clone, Copy)]
#[iden = "kept"]
struct Kept;

/// The name of the filters a walk tests each Entity of its selection by, one
/// row when the Entity passes and none otherwise.
#[derive(Iden, Clone, Copy)]
#[iden = "filtered"]
struct Filtered;

/// The statement a page is cut from: its relations, in the branch they are
/// read in, and the query and window it answers.
#[derive(Clone, Copy)]
pub(crate) struct PageSource<'a> {
    /// The relations the statement reads.
    pub(crate) relations: &'a EntityRelations,
    /// The branch schema, `None` for the live branch.
    pub(crate) schema: Option<&'a DynIden>,
    /// How the statement's fragments reach the rows.
    pub(crate) rows: &'a EntityRowRefs,
    /// The query.
    pub(crate) query: &'a CompiledEntityQuery,
    /// The window the page is.
    pub(crate) window: &'a PageWindow,
    /// The dictionary keys the query reads.
    pub(crate) names: &'a QueryNames,
}

impl PageSource<'_> {
    /// The page cut from the selection filtered by `condition` over `window`,
    /// projected, with the order it is read in.
    pub(crate) fn page(&self, condition: Condition, window: &PageWindow) -> Result<(ProjectedPage, ReadOrder), RenderError> {
        self.page_through(self.relations.clone(), condition, window)
    }

    /// The page cut from the selection read through `relations`, filtered by
    /// `condition` over `window`, projected, with the order it is read in.
    fn page_through(&self, relations: EntityRelations, condition: Condition, window: &PageWindow) -> Result<(ProjectedPage, ReadOrder), RenderError> {
        let mut page = Query::select();
        relations.apply_from(&mut page, self.schema, self.rows.standing());
        page.cond_where(condition);
        let (keys, read_order) = window_and_sort_keys(&mut page, self.rows, self.query, window, &self.names.attributes)?;
        Ok((projected_page(page, self.rows, keys, CountPolicy::Omit), read_order))
    }

    /// The selection filtered as `q` and `geo` say.
    pub(crate) fn condition(&self, q: QFilter<'_>, geo: GeoFilter) -> Result<Condition, RenderError> {
        build_entity_conditions(self.query, self.names, self.rows, &self.relations.source, q, geo)
    }

    /// The filters of [`PageSource::condition`], split as a walk reads them.
    pub(crate) fn conditions(&self, q: QFilter<'_>, geo: GeoFilter) -> Result<EntityConditions, RenderError> {
        entity_conditions(self.query, self.names, self.rows, &self.relations.source, q, geo)
    }

    /// How many matches the page needs from where the walk starts: every
    /// Entity an offset skips and the page itself, or the page past a cursor.
    pub(crate) fn need(&self) -> u64 {
        match self.window.start {
            PageStart::Offset(offset) => u64::from(offset) + u64::from(self.window.limit),
            PageStart::Cursor(_) => u64::from(self.window.limit),
        }
    }
}

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
    /// they read, and the rest, tested on each Entity it yields, so the short
    /// walk visits `short` Entities of the selection at most.
    pub(crate) walked: EntityConditions,
    /// The filter each visited Entity is probed for, over [`walked_rows`].
    pub(crate) probe: Condition,
    /// How many Entities the short walk visits.
    pub(crate) short: u32,
}

/// One read of a gated page's index step.
pub(crate) struct IndexRead {
    /// The candidates, unbounded: `SELECT "entity_key" …`.
    pub(crate) candidates: SelectStatement,
    /// The most candidates the read hands the page; past it the page tries
    /// the next read, and a read taking them whole is the last one.
    pub(crate) bound: CandidateBound,
    /// Every filter the candidates do not answer.
    pub(crate) narrowed: Condition,
}

/// What a gated page's rows carry beside the Entity's key and sort keys.
#[derive(Clone, Copy)]
pub(crate) struct GatedColumns {
    /// Whether the index branches' rows carry the total, the walks' a null
    /// one.
    pub(crate) total: CountPolicy,
    /// Whether each branch names itself on its rows.
    pub(crate) step: StepColumn,
}

/// The projected page of `source` cut by the gated steps: the short walk of
/// `walk`, each of `reads` in turn, and the full walk of `walk` when every
/// read passed its bound. Returns the order its rows come back in, its rows
/// carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn gated_page(
    source: &PageSource<'_>,
    walk: PageWalk<'_>,
    reads: Vec<IndexRead>,
    columns: GatedColumns,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let need = source.need();
    let (short, read_order) = short_walk(&walk, source.window, need)?;
    let ProjectedPage { select: short, order } = short;
    let mut ctes = WithClause::new();
    ctes.cte(materialized(short, ShortWalkMatches));

    let mut answered = Query::select();
    answered.column(Asterisk);
    named(&mut answered, columns, Total::Absent, GatedStep::ShortWalk);
    answered.from(ShortWalkMatches).and_where(count_of(ShortWalkMatches).gte(need));
    for (column, key_order) in &order {
        key_order.order_by(&mut answered, Expr::col(column.clone()));
    }
    cut(&mut answered, source.window);
    let mut union = wrapped(answered.take(), "answered");

    // Each later step runs only when every earlier one fell short of the page.
    let mut fell_short = vec![count_of(ShortWalkMatches).lt(need)];
    let mut walks = true;
    for (position, read) in reads.into_iter().enumerate() {
        let cte = Candidates::at(position);
        let candidates = narrowed_to(source.rows.entity_key_column(), column_of(cte.iden(), Entities::EntityKey));
        let mut narrowed = fell_short.iter().cloned().fold(read.narrowed.add(candidates), Condition::add);
        match read.bound {
            CandidateBound::AtMost(bound) => {
                narrowed = narrowed.add(within(cte.iden(), bound));
                fell_short.push(beyond(cte.iden(), bound));
            }
            CandidateBound::Whole => walks = false,
        }
        let (indexed, _) = source.page(narrowed, source.window)?;
        let mut indexed = indexed.select;
        named(&mut indexed, columns, Total::Counted, GatedStep::Candidates);
        union.union(UnionType::All, wrapped(indexed, "indexed"));
        ctes.cte(materialized(read.bound.read(read.candidates), cte.iden()));
        if !walks {
            break;
        }
    }
    if walks {
        let mut walk = wrapped(full_walk(walk, source.window, fell_short)?, "walk");
        named(&mut walk, columns, Total::Absent, GatedStep::FullWalk);
        union.union(UnionType::All, walk);
    }
    union.with_cte(ctes);
    Ok((ProjectedPage { select: union.take(), order }, read_order))
}

/// Whether a gated branch's rows carry the total.
#[derive(Clone, Copy)]
enum Total {
    /// The branch read every candidate, so it counts its rows before the
    /// window cuts them.
    Counted,
    /// The branch saw part of the selection: a null `bigint`, so the union's
    /// column takes the type of the index branches' count.
    Absent,
}

/// Project on `branch` what `columns` asks the rows of a gated page to carry:
/// the total as `total` says the branch can, and `step` as the step that
/// answered.
fn named(branch: &mut SelectStatement, columns: GatedColumns, total: Total, step: GatedStep) {
    match (columns.total, total) {
        (CountPolicy::Emit, Total::Counted) => {
            branch.expr_as(count_over_all(), Alias::new(TOTAL_COUNT_COLUMN));
        }
        (CountPolicy::Emit, Total::Absent) => {
            branch.expr_as(Expr::val(Value::BigInt(None)), Alias::new(TOTAL_COUNT_COLUMN));
        }
        (CountPolicy::Omit, Total::Counted | Total::Absent) => {}
    }
    match columns.step {
        StepColumn::Named => {
            branch.expr_as(step_value(step), Alias::new(GATED_STEP_COLUMN));
        }
        StepColumn::Unnamed => {}
    }
}

/// The `smallint` a branch names itself by.
fn step_value(step: GatedStep) -> SimpleExpr {
    Expr::val(Value::SmallInt(Some(step as i16)))
}

/// Cut `select`, ordered as the page is, to `window`: past its offset, or
/// from its cursor, which the selection it reads is already past.
fn cut(select: &mut SelectStatement, window: &PageWindow) {
    match window.start {
        PageStart::Offset(offset) => {
            select.offset(u64::from(offset));
        }
        PageStart::Cursor(_) => {}
    }
    select.limit(u64::from(window.limit));
}

/// The short walk: the first `walk.short` Entities of the selection in page
/// order, from where `window` starts, that the probe keeps, at most `need` of
/// them, and the order its rows come back in.
///
/// The probe filters the Entities the walk yields rather than the walk itself,
/// so the walk visits `walk.short` Entities at most whatever the probe
/// matches. A walk shorter than `need` cannot fill the page, so it visits
/// none.
fn short_walk(walk: &PageWalk<'_>, window: &PageWindow, need: u64) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let length = if need > u64::from(walk.short) { 0 } else { walk.short };
    let (mut matches, read_order) = walked_in_order(&walk.source, walk.walked.clone(), walk.probe.clone(), Vec::new(), window, Some(length))?;
    matches.select.limit(need);
    Ok((matches, read_order))
}

/// The full walk: every Entity of the selection in page order the probe
/// keeps, cut to `window`, behind the gates `fell_short` of every earlier
/// step.
fn full_walk(walk: PageWalk<'_>, window: &PageWindow, fell_short: Vec<SimpleExpr>) -> Result<SelectStatement, RenderError> {
    let (matches, _) = walked_in_order(&walk.source, walk.walked, walk.probe, fell_short, window, None)?;
    let mut matches = matches.select;
    cut(&mut matches, window);
    Ok(matches)
}

/// The selection `walked` keeps, read in page order from where `window`
/// starts, `length` Entities of it or every one, each it yields kept when
/// `probe` holds over [`walked_rows`], ordered as the page is, behind
/// `gates`.
///
/// The selection is a subquery behind its `LIMIT`, or behind `OFFSET 0`
/// without one, and the probe a lateral subquery behind `OFFSET 0`, which
/// keeps the planner from merging either into the other: it visits the
/// Entities in page order, probes each in turn, and stops once the statement
/// has its rows. The selection reads the walked relations
/// ([`EntityRelations::walked`]) by its selectors, so a search row it reads is
/// looked up for each Entity it yields, and tests the rest of `walked` on each
/// Entity in a lateral subquery of its own.
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
        source.relations.walked(),
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
    if !tested.is_empty() {
        selection.join_lateral(JoinType::InnerJoin, fenced(all_of(tested)), Filtered, Condition::all());
    }
    let mut matches = Query::select();
    matches
        .column((Walked, Asterisk))
        .from_subquery(selection, Walked)
        .join_lateral(JoinType::InnerJoin, fenced(probe), Kept, Condition::all());
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
