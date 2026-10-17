//! A page whose selection an index narrows by a term the planner cannot
//! price, cut in the order that stops soonest.
//!
//! Left the choice, the planner either walks the selection in page order and
//! filters each Entity, or reads the term's index and sorts what it yields,
//! and it misjudges which costs less by orders of magnitude when the matches
//! cluster in page order or its estimate of the term is off. The statement
//! therefore tries three ways, and a gate on what the previous one found
//! switches each later one off:
//!
//! 1. a short walk: the first Entities of the selection in page order, each
//!    probed for the filter. A term most Entities pass fills the page here.
//! 2. the index: the candidates read up to one past a bound, and the page cut
//!    from the selection narrowed to them when they number at most the bound;
//! 3. the full walk: the selection in page order, each Entity filtered, when
//!    more candidates qualify.
//!
//! Each step is one branch of a `UNION ALL` behind a
//! [`one_time_gate`](crate::one_time_gate). Every branch cuts the same window
//! over the same order, so whichever runs returns the page the plain
//! statement would.
//!
//! A counted page's index branch carries the total (clause 6.3.13) as
//! `COUNT(*) OVER ()`: it reads every candidate before it cuts the window,
//! so the count costs no second read of them. The other two branches carry
//! no total, the short walk having seen part of the selection and the full
//! walk stopping once the page is cut. Each branch can also name itself on
//! its rows ([`GatedStep`]), so the count run when the page carries no total
//! knows what the page found.

use crate::{
    entity::{QFilter, build_entity_conditions, window_and_sort_keys},
    entity_page::{GATED_STEP_COLUMN, GatedStep, ProjectedPage, StepColumn, TOTAL_COUNT_COLUMN, projected_page},
    entity_relations::EntityRelations,
    one_time_gate::{CandidateBound, Candidates, column_of, count_of, materialized, within},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{dictionary::query_names::QueryNames, entity_row_refs::EntityRowRefs, error::RenderError};
use andromeda_postgresql_seaquery::aggregate::count_over_all;
use sea_query::{Alias, Asterisk, Condition, DynIden, Expr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr, UnionType, Value, WithClause};

/// The CTE holding the short walk's matches.
#[derive(Iden, Clone, Copy)]
#[iden = "short_walk"]
struct ShortWalkMatches;

/// The name the short walk reads the Entities it probes under.
#[derive(Iden, Clone, Copy)]
#[iden = "walked"]
pub(crate) struct Walked;

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
        let mut page = Query::select();
        self.relations.clone().apply_from(&mut page, self.schema);
        page.cond_where(condition);
        let (keys, read_order) = window_and_sort_keys(&mut page, self.rows, self.query, window, &self.names.attributes)?;
        Ok((projected_page(page, self.rows, keys, CountPolicy::Omit), read_order))
    }

    /// The selection filtered as `q` says.
    pub(crate) fn condition(&self, q: QFilter<'_>) -> Result<Condition, RenderError> {
        build_entity_conditions(self.query, self.names, self.rows, &self.relations.source, q)
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
/// walked Entity.
pub(crate) fn walked_rows(schema: Option<&DynIden>) -> EntityRowRefs {
    EntityRowRefs::unaliased(schema.cloned()).keyed_by(Walked)
}

/// The first step of a gated page.
pub(crate) struct ShortWalk<'a> {
    /// What the walk reads: the selection's relations, or fewer when the
    /// probe reads the rest.
    pub(crate) source: PageSource<'a>,
    /// The filter the walk keeps every Entity it visits by, which must not
    /// narrow the selection, or the walk would visit more than `length`
    /// Entities to find them.
    pub(crate) walked: Condition,
    /// The filter each visited Entity is probed for, over [`walked_rows`].
    pub(crate) probe: Condition,
    /// How many Entities the walk visits.
    pub(crate) length: u32,
}

/// The second and third steps of a gated page.
pub(crate) struct IndexOrWalk {
    /// The candidates, unbounded: `SELECT "entity_key" …`.
    pub(crate) candidates: SelectStatement,
    /// The most candidates the index step reads; past it the full walk
    /// cuts the page, and a page reading them whole has no full walk.
    pub(crate) bound: CandidateBound,
    /// Every filter the candidates do not answer, for the index step.
    pub(crate) narrowed: Condition,
    /// Every filter, for the full walk.
    pub(crate) whole: Condition,
}

/// What a gated page's rows carry beside the Entity's key and sort keys.
#[derive(Clone, Copy)]
pub(crate) struct GatedColumns {
    /// Whether the index branch's rows carry the total, the other branches'
    /// a null one.
    pub(crate) total: CountPolicy,
    /// Whether each branch names itself on its rows.
    pub(crate) step: StepColumn,
}

/// The projected page of `source` cut by the three gated steps, and the order
/// its rows come back in, its rows carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn gated_page(
    source: &PageSource<'_>,
    short: ShortWalk<'_>,
    rest: IndexOrWalk,
    columns: GatedColumns,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let need = source.need();
    let short = short_walk(short, source.window, need)?;
    let walked_enough = count_of(ShortWalkMatches).gte(need);
    let walked_short = count_of(ShortWalkMatches).lt(need);

    let mut narrowed = rest
        .narrowed
        .add(source.rows.entity_key_column().in_subquery(column_of(Candidates)))
        .add(walked_short.clone()); // clone: both later branches are gated on the short walk
    let mut walk = match rest.bound {
        CandidateBound::AtMost(bound) => {
            narrowed = narrowed.add(within(Candidates, bound));
            let (walk, _) = source.page(rest.whole.add(walked_short).add(count_of(Candidates).gt(bound)), source.window)?;
            Some(wrapped(walk.select, "walk"))
        }
        CandidateBound::Whole => None,
    };
    let (indexed, read_order) = source.page(narrowed, source.window)?;
    let ProjectedPage { select: mut indexed, order } = indexed;

    let mut answered = Query::select();
    answered.column(Asterisk);
    match columns.total {
        CountPolicy::Emit => {
            indexed.expr_as(count_over_all(), Alias::new(TOTAL_COUNT_COLUMN));
            answered.expr_as(no_total(), Alias::new(TOTAL_COUNT_COLUMN));
            if let Some(walk) = walk.as_mut() {
                walk.expr_as(no_total(), Alias::new(TOTAL_COUNT_COLUMN));
            }
        }
        CountPolicy::Omit => {}
    }
    match columns.step {
        StepColumn::Named => {
            answered.expr_as(step(GatedStep::ShortWalk), Alias::new(GATED_STEP_COLUMN));
            indexed.expr_as(step(GatedStep::Candidates), Alias::new(GATED_STEP_COLUMN));
            if let Some(walk) = walk.as_mut() {
                walk.expr_as(step(GatedStep::FullWalk), Alias::new(GATED_STEP_COLUMN));
            }
        }
        StepColumn::Unnamed => {}
    }
    answered.from(ShortWalkMatches).and_where(walked_enough);
    for (column, key_order) in &order {
        key_order.order_by(&mut answered, Expr::col(column.clone())); // clone: each branch orders by the same columns
    }
    match source.window.start {
        PageStart::Offset(offset) => {
            answered.offset(u64::from(offset));
        }
        PageStart::Cursor(_) => {}
    }
    answered.limit(u64::from(source.window.limit));

    let mut union = wrapped(answered.take(), "answered");
    union.union(UnionType::All, wrapped(indexed, "indexed"));
    if let Some(walk) = walk {
        union.union(UnionType::All, walk);
    }
    union.with_cte(
        WithClause::new()
            .cte(materialized(short, ShortWalkMatches))
            .cte(materialized(rest.bound.read(rest.candidates), Candidates))
            .to_owned(),
    );
    Ok((ProjectedPage { select: union.take(), order }, read_order))
}

/// The total a branch that cannot count carries: a null `bigint`, so the
/// union's column takes the type of the index branch's count.
fn no_total() -> SimpleExpr {
    Expr::val(Value::BigInt(None))
}

/// The `smallint` a branch names itself by.
fn step(step: GatedStep) -> SimpleExpr {
    Expr::val(Value::SmallInt(Some(step as i16)))
}

/// The short walk: the first `length` Entities of the selection in page order,
/// from where `window` starts, that the probe keeps, at most `need` of them.
///
/// The probe filters the Entities the walk yields rather than the walk itself,
/// so the walk visits `length` Entities at most whatever the probe matches. A
/// walk shorter than `need` cannot fill the page, so it visits none.
fn short_walk(short: ShortWalk<'_>, window: &PageWindow, need: u64) -> Result<SelectStatement, RenderError> {
    let start = match &window.start {
        PageStart::Offset(_) => PageStart::Offset(0),
        // clone: the walk starts where the window does
        PageStart::Cursor(cursor) => PageStart::Cursor(cursor.clone()),
    };
    let limit = if need > u64::from(short.length) { 0 } else { short.length };
    let window = PageWindow { limit, start };
    let (walked, _) = short.source.page(short.walked, &window)?;
    let ProjectedPage { select: walked, order } = walked;
    let mut matches = Query::select();
    matches.column((Walked, Asterisk)).from_subquery(walked, Walked).cond_where(short.probe);
    for (column, key_order) in order {
        key_order.order_by(&mut matches, Expr::col((Walked, column)));
    }
    matches.limit(need);
    Ok(matches.take())
}

/// `SELECT * FROM (<branch>) AS <alias>`, so the branch keeps its own order
/// and window inside the union.
pub(crate) fn wrapped(branch: SelectStatement, alias: &str) -> SelectStatement {
    Query::select().column(Asterisk).from_subquery(branch, Alias::new(alias)).take()
}
