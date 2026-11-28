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
//! 1. the leading reads, one term after another, each counting the index
//!    entries that bound its candidates up to one past the short walk's
//!    length and, when they number at most that length, cutting the page from
//!    the selection narrowed to its candidates. A term so rare never pays a
//!    walk;
//! 2. a short walk of the first Entities of the selection in page order,
//!    each probed for the filter. A term most Entities pass fills the page
//!    here;
//! 3. on a page that has one, a continued walk of the next Entities in page
//!    order, when the short walk's matches project the page full within them;
//! 4. the trailing reads, one term after another, each reading its
//!    candidates up to one past a bound and cutting the page from the
//!    selection narrowed to them when they number at most the bound;
//! 5. the full walk of the selection in page order, each Entity probed for
//!    the filter, when every trailing read found more candidates than its
//!    bound.
//!
//! Each step is one branch of a `UNION ALL` behind a
//! [`one_time_gate`](andromeda_postgresql_query::one_time_gate). Every branch cuts the same window
//! over the same order, so whichever runs returns the page the plain
//! statement would.
//!
//! The walks are a [`PageWalk`]'s. An index branch narrows the selection to
//! its candidates as an array
//! ([`narrowed_to`](andromeda_postgresql_query::one_time_gate::narrowed_to)),
//! looked up one by one, and keeps the plain join of the search row and the
//! filters, since it looks each candidate up by its key.
//!
//! A counted page's index branch carries the total (clause 6.3.13) as
//! `COUNT(*) OVER ()`: it reads every candidate before it cuts the window,
//! so the count costs no second read of them. The walks carry no total, the
//! short and continued walks having seen part of the selection and the full
//! walk stopping once the page is cut. Each branch can also name itself on
//! its rows ([`GatedStep`]), so the count run when the page carries no total
//! knows what the page found.

use crate::{
    entity::{EntityConditions, GeoFilter, QFilter, build_entity_conditions, selection_conditions, tested_conditions, window_and_sort_keys},
    entity_page::{GatedStep, PageTotal, ProjectedPage, StepColumn, projected_page},
    entity_relations::EntityRelations,
    gated_steps::{GatedSteps, Total, named},
    page_walk::{PageWalk, continued_walk, full_walk, short_walk, walked_rows},
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
    one_time_gate::{CandidateBound, ContinuedWalkMatches, IndexEntries, ShortWalkMatches, count_of, materialized, wrapped},
    query_names::QueryNames,
};
use sea_query::{Condition, DynIden, ExprTrait, Query, SelectStatement, UnionType};

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
    ///
    /// A type walk of several alternatives is read as the union of its
    /// streams, which the page sorts whole: it is for a selection narrowed to
    /// few candidates, or one ordered by keys no stream is read in.
    pub(crate) fn page(&self, condition: Condition, window: &PageWindow) -> Result<(ProjectedPage, ReadOrder), RenderError> {
        self.page_through(self.relations.clone(), condition, window, PageTotal::Omitted)
    }

    /// The page cut from the selection read through `relations`, filtered by
    /// `condition` over `window`, projected and carrying `total`, with the
    /// order it is read in.
    pub(crate) fn page_through(
        &self,
        relations: EntityRelations,
        condition: Condition,
        window: &PageWindow,
        total: PageTotal,
    ) -> Result<(ProjectedPage, ReadOrder), RenderError> {
        let mut page = Query::select();
        relations.apply_from(&mut page, self.schema, self.rows.standing());
        page.cond_where(condition);
        let (keys, read_order) = window_and_sort_keys(&mut page, self.rows, self.query, window, &self.names.attributes)?;
        Ok((projected_page(page, self.rows, keys, total), read_order))
    }

    /// The selection filtered as `q` and `geo` say.
    pub(crate) fn condition(&self, q: QFilter<'_>, geo: GeoFilter) -> Result<Condition, RenderError> {
        build_entity_conditions(self.query, self.names, self.rows, &self.relations.source, q, geo)
    }

    /// The filters of [`PageSource::condition`], split as a walk reads them:
    /// the selection over the statement's rows, and the rest over
    /// [`walked_rows`], each Entity the walk yields.
    pub(crate) fn conditions(&self, q: QFilter<'_>, geo: GeoFilter) -> Result<EntityConditions, RenderError> {
        Ok(EntityConditions {
            selection: selection_conditions(self.query, self.names, self.rows, &self.relations.source),
            tested: tested_conditions(self.query, self.names, &walked_rows(self.rows), q, geo)?,
        })
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

/// A read a gated page tries before its walks, which counts the index
/// entries bounding a term's candidates up to one past the short walk's
/// length.
///
/// Within that length the candidates number no more, and cutting the page
/// from them probes no more Entities than the short walk would, so the page
/// reads them and answers without walking. Past it the page walks, having
/// read no candidate.
pub(crate) struct LeadingRead {
    /// The index entries bounding the candidates, unbounded, as `SELECT * …`.
    pub(crate) entries: SelectStatement,
    /// The candidates, unbounded, as `SELECT "entity_key" …`.
    pub(crate) candidates: SelectStatement,
    /// Every filter the candidates do not answer.
    pub(crate) narrowed: Condition,
}

/// The index reads of a gated page, by where they stand beside its walks.
pub(crate) struct GatedReads {
    /// The reads tried before the walks, in turn.
    pub(crate) leading: Vec<LeadingRead>,
    /// The reads tried in turn once the walks fell short of the page.
    pub(crate) trailing: Vec<IndexRead>,
}

impl GatedReads {
    /// `trailing` alone, every read tried once the walks fell short.
    pub(crate) const fn after_walks(trailing: Vec<IndexRead>) -> GatedReads {
        GatedReads { leading: Vec::new(), trailing }
    }
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

/// The projected page of `source` cut by the gated steps, which are each of
/// the leading `reads` in turn, the short walk of `walk` and its continued
/// walk, each of the trailing `reads` in turn, and the full walk of `walk`
/// when every trailing read passed its bound. Returns the order its rows come
/// back in, its rows carrying what `columns` asks for.
///
/// # Errors
///
/// Fails as rendering the plain page fails: a reference geometry with no EWKB
/// spelling, or a query-language rule a renderer decides first.
pub(crate) fn gated_page(
    source: &PageSource<'_>,
    walk: PageWalk<'_>,
    reads: GatedReads,
    columns: GatedColumns,
) -> Result<(ProjectedPage, ReadOrder), RenderError> {
    let need = source.need();
    let (short, read_order) = short_walk(&walk, source.window, need)?;
    let ProjectedPage { select: short, order } = short;
    let continued = continued_walk(&walk, source.window, need, &order)?;
    let mut steps = GatedSteps::new(source, columns);
    let leading = reads
        .leading
        .into_iter()
        .enumerate()
        .map(|(position, read)| steps.leading(IndexEntries::at(position), read, u64::from(walk.short)))
        .collect::<Result<Vec<SelectStatement>, RenderError>>()?;

    steps.ctes.cte(materialized(short, ShortWalkMatches));
    let filled = vec![count_of(ShortWalkMatches).gte(need)];
    let mut union = steps.walked(ShortWalkMatches, filled, &order, GatedStep::ShortWalk, "answered");
    for branch in leading {
        union.union(UnionType::All, branch);
    }
    match continued {
        Some(continued) => {
            steps.ctes.cte(materialized(continued, ContinuedWalkMatches));
            let filled = vec![count_of(ShortWalkMatches).lt(need), count_of(ContinuedWalkMatches).gte(need)];
            union.union(
                UnionType::All,
                steps.walked(ContinuedWalkMatches, filled, &order, GatedStep::ContinuedWalk, "continued"),
            );
            steps.fell_short.push(count_of(ContinuedWalkMatches).lt(need));
        }
        None => steps.fell_short.push(count_of(ShortWalkMatches).lt(need)),
    }

    let mut walks = true;
    for read in reads.trailing {
        let bound = read.bound;
        union.union(UnionType::All, steps.indexed(read.candidates, read.narrowed, bound)?);
        if bound == CandidateBound::Whole {
            walks = false;
            break;
        }
    }
    if walks {
        let mut walk = wrapped(full_walk(walk, source.window, &steps.fell_short)?, "walk");
        named(&mut walk, columns, Total::Absent, GatedStep::FullWalk);
        union.union(UnionType::All, walk);
    }
    union.with_cte(steps.ctes);
    Ok((ProjectedPage { select: union.take(), order }, read_order))
}

/// Cut `select`, ordered as the page is, to the rows of `window` past its
/// first `skipped`: past its offset and `skipped`, or past `skipped` from its
/// cursor, which the selection it reads is already past.
pub(crate) fn cut(select: &mut SelectStatement, window: &PageWindow, skipped: u64) {
    match window.start {
        PageStart::Offset(offset) => {
            select.offset(u64::from(offset).saturating_add(skipped));
        }
        PageStart::Cursor(_) if skipped > 0 => {
            select.offset(skipped);
        }
        PageStart::Cursor(_) => {}
    }
    select.limit(u64::from(window.limit));
}
