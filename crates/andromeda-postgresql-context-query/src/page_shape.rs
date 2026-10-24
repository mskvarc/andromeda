//! How the page of an entity query is cut from its selection: one plain
//! statement the planner plans, or a gated page whose index the query's
//! terms give it.

use crate::{
    carrier_page::{carrier_page, carriers},
    entity::window_and_sort_keys,
    entity_page::{StepColumn, heads_of, heads_over},
    entity_relations::EntityRelations,
    entity_total::TotalCandidates,
    gated_page::{GatedColumns, PageSource},
    id_pattern_page::{PatternMatches, id_pattern_page, pattern_matches},
    keyed_page::keyed_page,
    value_walk_page::{ValueWalkOrder, value_walk_order, value_walk_page},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::expression::Expr as QueryExpr,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    q_predicate::keyed_term::{KeyedCandidates, keyed_candidates},
};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use sea_query::{Condition, DynIden, Query, SelectStatement};

/// How a page is cut from its selection.
pub(crate) enum PageShape<'q> {
    /// One statement over the selection, ordered and windowed, the plan left
    /// to the planner.
    Plain,
    /// The gated page of a `q` whose conjunct an index answers.
    Keyed(&'q QueryExpr, KeyedCandidates<'q>),
    /// The gated page of a selection by `attrs` alone, the statement reading
    /// the Entities that carry one of the Attributes.
    Carriers(SelectStatement),
    /// The gated page of a selection ordered by one Attribute, walked in the
    /// order of its value index while every instance of it holds a number, or
    /// while every instance holds a `DateTime`.
    ValueWalk(ValueWalkOrder),
    /// The gated page of a type selection filtered by an unanchored
    /// `idPattern` alone, the statement reading the pattern's trigram matches.
    IdPattern(PatternMatches<'q>),
}

/// A page cut from its selection, and what its total reads.
pub(crate) struct CutPage<'q> {
    /// The statement selecting the heads of the page's Entities.
    pub(crate) select: SelectStatement,
    /// The order the page's rows come back in.
    pub(crate) read_order: ReadOrder,
    /// How PostgreSQL may plan and run `select`.
    pub(crate) planning: StatementPlanning,
    /// How PostgreSQL may plan the statement the total runs.
    pub(crate) total_planning: StatementPlanning,
    /// The candidates the total reads its matches from, `None` when it
    /// counts the selection under the whole filter. A page with candidates
    /// names on its rows the gated step that answered it.
    pub(crate) total_candidates: Option<TotalCandidates<'q>>,
}

impl<'q> PageShape<'q> {
    /// How the page of `query` over `window` is cut: gated when a term of
    /// `q`, the request's `attrs`, an ordering a value walk serves or an unanchored
    /// `idPattern` gives the page an index to read, plain otherwise.
    pub(crate) fn of(
        query: &'q CompiledEntityQuery,
        names: &QueryNames,
        window: &PageWindow,
        relations: &EntityRelations,
        rows: &EntityRowRefs,
        schema: Option<&DynIden>,
    ) -> PageShape<'q> {
        let keyed = query
            .selection
            .q_expr
            .as_ref()
            .and_then(|q_expr| keyed_candidates(q_expr, rows, names, query.selection.language_matching).map(|keyed| (q_expr, keyed)));
        match keyed {
            Some((q_expr, keyed)) => PageShape::Keyed(q_expr, keyed),
            None => carriers(query, names, schema)
                .map(PageShape::Carriers)
                .or_else(|| value_walk_order(query, names, window).map(PageShape::ValueWalk))
                .or_else(|| pattern_matches(query, relations, schema).map(PageShape::IdPattern))
                .unwrap_or(PageShape::Plain),
        }
    }

    /// Whether the page's rows carry the total `count` asks for.
    ///
    /// An offset page, and the first page of a cursor walk, piggyback a
    /// requested total on their rows: every row of a plain page, the rows of
    /// a gated page whose index branch answered, which read every candidate.
    /// A continued cursor page cannot, since its rows are only those past the
    /// cursor, and neither can a value walk page, whose branches each read part
    /// of the selection.
    pub(crate) const fn counted_on_page(&self, count: CountPolicy, window: &PageWindow) -> CountPolicy {
        match (count, &window.start, self) {
            (
                CountPolicy::Emit,
                PageStart::Offset(_) | PageStart::Cursor(None),
                PageShape::Plain | PageShape::Keyed(..) | PageShape::Carriers(_) | PageShape::IdPattern(_),
            ) => CountPolicy::Emit,
            (CountPolicy::Emit, PageStart::Cursor(Some(_)), PageShape::Plain | PageShape::Keyed(..) | PageShape::Carriers(_) | PageShape::IdPattern(_))
            | (CountPolicy::Emit, PageStart::Offset(_) | PageStart::Cursor(_), PageShape::ValueWalk(_))
            | (
                CountPolicy::Omit,
                PageStart::Offset(_) | PageStart::Cursor(_),
                PageShape::Plain | PageShape::Keyed(..) | PageShape::Carriers(_) | PageShape::ValueWalk(_) | PageShape::IdPattern(_),
            ) => CountPolicy::Omit,
        }
    }

    /// The page of `source` cut this way, the plain page filtered by
    /// `condition`, its rows carrying the total when `counted_on_page` says
    /// so, and the candidates a total `count` asks for reads.
    ///
    /// # Errors
    ///
    /// Fails as rendering the page's filter or ordering fails: a reference
    /// geometry with no EWKB spelling, or a query-language rule a renderer
    /// decides first.
    pub(crate) fn cut(
        self,
        source: &PageSource<'_>,
        condition: &Condition,
        count: CountPolicy,
        counted_on_page: CountPolicy,
    ) -> Result<CutPage<'q>, RenderError> {
        let (page, read_order, total_planning, total_candidates) = match self {
            PageShape::Plain => {
                let mut page = Query::select();
                // clone: the source lends its relations, and applying them takes them
                source.relations.clone().apply_from(&mut page, source.schema);
                // clone: the count statement filters on the same condition.
                page.cond_where(condition.clone());
                let (keys, read_order) = window_and_sort_keys(&mut page, source.rows, source.query, source.window, &source.names.attributes)?;
                return Ok(CutPage {
                    select: heads_of(page, source.rows, keys, counted_on_page),
                    read_order,
                    planning: StatementPlanning::Cached,
                    total_planning: StatementPlanning::Cached,
                    total_candidates: None,
                });
            }
            PageShape::Keyed(q_expr, keyed) => {
                let keyed_total = match count {
                    // clone: the count reads the same candidates under its own bound
                    CountPolicy::Emit => Some(TotalCandidates::Term(KeyedCandidates {
                        select: keyed.select.clone(),
                        ..keyed
                    })),
                    CountPolicy::Omit => None,
                };
                let (page, read_order) = keyed_page(source, q_expr, keyed, gated_columns(count, counted_on_page))?;
                (page, read_order, StatementPlanning::BoundValues, keyed_total)
            }
            PageShape::Carriers(carriers) => {
                let (page, read_order) = carrier_page(source, carriers, counted_on_page)?;
                (page, read_order, StatementPlanning::Cached, None)
            }
            PageShape::ValueWalk(order) => {
                let (page, read_order) = value_walk_page(source, &order)?;
                (page, read_order, StatementPlanning::Cached, None)
            }
            PageShape::IdPattern(matches) => {
                let matches_total = match count {
                    // clone: the count reads the same matches under its own bound
                    CountPolicy::Emit => Some(TotalCandidates::IdPattern(matches.select.clone())),
                    CountPolicy::Omit => None,
                };
                let (page, read_order) = id_pattern_page(source, matches, gated_columns(count, counted_on_page))?;
                (page, read_order, StatementPlanning::BoundValues, matches_total)
            }
        };
        let step = match total_candidates {
            Some(_) => StepColumn::Named,
            None => StepColumn::Unnamed,
        };
        Ok(CutPage {
            select: heads_over(page, source.rows, counted_on_page, step),
            read_order,
            planning: StatementPlanning::GatedBranches,
            total_planning,
            total_candidates,
        })
    }
}

/// What the rows of a gated page whose total reads candidates carry: the
/// total when `counted_on_page` says the page can, and the step that answered
/// it whenever `count` asks for a total, so a count run on its own skips the
/// candidates of a page the short walk filled.
const fn gated_columns(count: CountPolicy, counted_on_page: CountPolicy) -> GatedColumns {
    GatedColumns {
        total: counted_on_page,
        step: match count {
            CountPolicy::Emit => StepColumn::Named,
            CountPolicy::Omit => StepColumn::Unnamed,
        },
    }
}
