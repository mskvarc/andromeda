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
    geo_page::{GeoMatches, geo_matches, geo_page},
    id_pattern_page::{PatternMatches, id_pattern_page, pattern_matches},
    keyed_page::{keyed_page, page_bound},
    value_walk_page::{ValueWalkOrder, value_walk_order, value_walk_page},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::compiled_filter::CompiledFilter,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    q_predicate::keyed_term::{KeyedCandidates, keyed_candidates},
    query_names::QueryNames,
};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use sea_query::{Condition, DynIden, Query, SelectStatement};

/// How a page is cut from its selection.
pub(crate) enum PageShape<'q> {
    /// One statement over the selection, ordered and windowed, the plan left
    /// to the planner.
    Plain,
    /// The gated page of a `q` whose conjuncts an index answers, read in the
    /// order listed.
    Keyed(&'q CompiledFilter, Vec<KeyedCandidates<'q>>),
    /// The gated page of a selection filtered by a geo-query, the statement
    /// reading the geo-query's matches.
    Geo(GeoMatches<'q>),
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
    /// How the page of `query` over `window` is cut when `count` asks for its
    /// total: gated when a term of `q`, a geo-query, the request's `attrs`, an
    /// ordering a value walk serves or an unanchored `idPattern` gives the
    /// page an index to read, plain otherwise.
    ///
    /// # Errors
    ///
    /// Fails as rendering the matches of a geo-query fails: a reference
    /// geometry with no EWKB spelling, or `georel=near` naming no distance.
    pub(crate) fn of(
        query: &'q CompiledEntityQuery,
        names: &QueryNames,
        window: &PageWindow,
        count: CountPolicy,
        relations: &EntityRelations,
        rows: &EntityRowRefs,
        schema: Option<&DynIden>,
    ) -> Result<PageShape<'q>, RenderError> {
        let keyed = query
            .selection
            .q
            .as_ref()
            .map(|q| (q, keyed_candidates(q.reading(), rows, names, page_bound)))
            .filter(|(_, keyed)| !keyed.is_empty());
        if let Some((q, keyed)) = keyed {
            return Ok(PageShape::Keyed(q, keyed));
        }
        if let Some(matches) = geo_matches(query, names, window, count, rows)? {
            return Ok(PageShape::Geo(matches));
        }
        Ok(carriers(query, names, schema)
            .map(PageShape::Carriers)
            .or_else(|| value_walk_order(query, names, window).map(PageShape::ValueWalk))
            .or_else(|| pattern_matches(query, relations, schema).map(PageShape::IdPattern))
            .unwrap_or(PageShape::Plain))
    }

    /// Whether the page's rows carry the total `count` asks for.
    ///
    /// A plain page and a gated page carry it as [`carried_on_rows`] says. A
    /// value walk page cannot, since its branches each read part of the
    /// selection.
    pub(crate) const fn counted_on_page(&self, count: CountPolicy, window: &PageWindow) -> CountPolicy {
        match self {
            PageShape::Plain | PageShape::Keyed(..) | PageShape::Geo(_) | PageShape::Carriers(_) | PageShape::IdPattern(_) => carried_on_rows(count, window),
            PageShape::ValueWalk(_) => CountPolicy::Omit,
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
                source.relations.clone().apply_from(&mut page, source.schema, source.rows.standing());
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
            PageShape::Keyed(q, keyed) => {
                let keyed_total = match count {
                    CountPolicy::Emit => Some(TotalCandidates::Terms(keyed.clone())),
                    CountPolicy::Omit => None,
                };
                let (page, read_order) = keyed_page(source, q, keyed, gated_columns(count, counted_on_page))?;
                (page, read_order, StatementPlanning::BoundValues, keyed_total)
            }
            PageShape::Geo(matches) => {
                // The total is the plain count of the whole filter whichever
                // step answers, so the rows name none.
                let columns = GatedColumns {
                    total: counted_on_page,
                    step: StepColumn::Unnamed,
                };
                let (page, read_order) = geo_page(source, matches, columns)?;
                (page, read_order, StatementPlanning::Cached, None)
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
                    CountPolicy::Emit => Some(TotalCandidates::IdPattern(Box::new(matches.select.clone()))),
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

/// The total `count` asks for, when a page that reads every match before it
/// cuts its window can carry it on its rows.
///
/// An offset page and the first page of a cursor walk carry it, on every row
/// of a plain page and on the rows of a gated page whose index branch
/// answered, since that branch read every candidate. A continued cursor page
/// cannot, since its rows are only those past the cursor.
pub(crate) const fn carried_on_rows(count: CountPolicy, window: &PageWindow) -> CountPolicy {
    match (count, &window.start) {
        (CountPolicy::Emit, PageStart::Offset(_) | PageStart::Cursor(None)) => CountPolicy::Emit,
        (CountPolicy::Emit, PageStart::Cursor(Some(_))) | (CountPolicy::Omit, PageStart::Offset(_) | PageStart::Cursor(_)) => CountPolicy::Omit,
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

#[cfg(test)]
mod tests {
    use crate::{
        entity_relations::EntityRelations,
        entity_total::EntityTotal,
        geo_page_fixtures::{built, filtered, first_page, names, past_first_sensor, sensors_within_square},
        page_shape::PageShape,
    };
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::ConsideredEntities,
        filter::path::AttrPath,
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        page_window::{PageStart, PageWindow},
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use pretty_assertions::assert_eq;

    /// How the page of `query` over `window` is cut when `count` asks for its
    /// total, on the live branch.
    fn shape_of<'q>(query: &'q CompiledEntityQuery, window: &PageWindow, count: CountPolicy) -> PageShape<'q> {
        let names = names();
        let relations = EntityRelations::for_query(query, &names.types, None);
        let rows = relations.rows(None, query.selection.standing);
        PageShape::of(query, &names, window, count, &relations, &rows, None).expect("valid entity query")
    }

    #[test]
    fn a_geo_query_alone_is_cut_by_the_geo_page() {
        assert!(matches!(
            shape_of(&sensors_within_square(), &first_page(), CountPolicy::Omit),
            PageShape::Geo(_)
        ));
    }

    #[test]
    fn a_geo_query_beside_a_keyed_term_keeps_the_keyed_page() {
        let query = filtered(sensors_within_square(), "\"https://example.com/status\"==\"fail\"");
        assert!(matches!(shape_of(&query, &first_page(), CountPolicy::Omit), PageShape::Keyed(..)));
    }

    #[test]
    fn a_geo_query_under_an_ordering_keeps_the_plain_page() {
        let mut query = sensors_within_square();
        query.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple("https://example.com/temperature".parse().expect("valid attribute name"))),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        assert!(matches!(shape_of(&query, &first_page(), CountPolicy::Omit), PageShape::Plain));
    }

    /// Counting the whole selection costs what the plain page costs, so an
    /// offset page or the first page of a cursor walk, whose rows can carry
    /// the total, stays plain.
    #[test]
    fn a_counted_geo_page_that_carries_its_total_keeps_the_plain_page() {
        let first_cursor_page = PageWindow {
            limit: 100,
            start: PageStart::Cursor(None),
        };
        for window in [first_page(), first_cursor_page] {
            assert!(matches!(shape_of(&sensors_within_square(), &window, CountPolicy::Emit), PageShape::Plain));
        }
    }

    /// Clause 6.3.13: past a cursor the total is a count of its own whatever
    /// cuts the page, the count of the whole filter the plain page carries.
    #[test]
    fn a_counted_continued_cursor_page_of_a_geo_query_is_gated() {
        let query = sensors_within_square();
        assert!(matches!(shape_of(&query, &past_first_sensor(), CountPolicy::Emit), PageShape::Geo(_)));
        let EntityTotal::Counted(continued) = built(&query, &past_first_sensor(), CountPolicy::Emit).total else {
            panic!("past a cursor the total is a count of its own");
        };
        let EntityTotal::OnPageRows { uncounted: plain } = built(&query, &first_page(), CountPolicy::Emit).total else {
            panic!("a counted first page carries its total");
        };
        assert!(continued.walked.is_none());
        assert_eq!(continued.count, plain.count);
        assert_statement_snapshot!("page-shape-a-counted-continued-cursor-page-of-a-geo-query-is-gated", continued.count);
    }

    #[test]
    fn a_geo_query_on_an_unrecorded_geoproperty_keeps_the_plain_page() {
        let mut query = sensors_within_square();
        query.selection.geo.as_mut().expect("a geo-query").geoproperty = "https://example.com/neverRecorded".parse().expect("valid attribute name");
        assert!(matches!(shape_of(&query, &first_page(), CountPolicy::Omit), PageShape::Plain));
    }

    #[test]
    fn a_selection_of_no_entity_keeps_the_plain_page() {
        let mut query = sensors_within_square();
        query.selection.considered = ConsideredEntities::NoEntity;
        assert!(matches!(shape_of(&query, &first_page(), CountPolicy::Omit), PageShape::Plain));
    }
}
