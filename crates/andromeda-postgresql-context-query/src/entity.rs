//! SQL generation for entity queries using sea-query.

use crate::{
    carried_names::{attrs_condition, projected_member_condition},
    entity_relations::{EntityRelations, RowSource},
    entity_total::{EntityTotal, TotalSource, entity_total},
    gated_page::PageSource,
    page_shape::PageShape,
    selector_predicate::condition as selector_condition,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    entity_selection::ConsideredEntities,
    filter::reading::term_reading::TermReading,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
    scope::scope_query::ScopeQuery,
};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    geo_predicate as geo_sql,
    ordering::result_ordering,
    page_window::bound_to_window,
    q_predicate::{instance_scan::InstanceScan, projection as q_filter},
    query_names::QueryNames,
    scope_predicate::{ScopeTarget, condition as scope_condition},
    sort_key::{KeyOrder, NullPlacement, SortKey},
};
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{literal::false_literal, statement_planning::StatementPlanning};
use andromeda_postgresql_stored_values::resolved_names::ResolvedNames;
use sea_query::{Condition, Query, SelectStatement};

/// A compiled entity query: the paginated result statement and how the total
/// it reports is read, each a statement tree the store binds and executes on
/// its own.
pub struct EntityQuery {
    /// The paginated statement selecting the heads of the matching entities,
    /// whose `current_attributes` rows the store reads by their keys.
    pub select: SelectStatement,
    /// How the clause 6.3.13 total is read, when the query asked for one.
    pub total: EntityTotal,
    /// The order the page's rows come back in, which the store presents
    /// ascending.
    pub read_order: ReadOrder,
    /// How PostgreSQL may plan and run `select`: a gated page, keyed, cut
    /// from the carriers of its `attrs` or the trigram matches of its
    /// `idPattern`, or walked in the order of an Attribute's value index, with its
    /// bound values, which alone let the planner price an
    /// index against walking the selection, and without the parallel workers
    /// a gated-off branch would launch.
    pub planning: StatementPlanning,
    /// How PostgreSQL may plan the statement `total` runs: a keyed count with
    /// its bound values, for the same pricing.
    pub total_planning: StatementPlanning,
}

/// A compiled identifier-only entity query.
pub struct EntityIdsQuery {
    /// The paginated statement selecting the matching entity identifiers.
    pub select: SelectStatement,
    /// The order the page's identifiers come back in, which the store presents
    /// ascending.
    pub read_order: ReadOrder,
}

/// Builds a parameterized SQL query to retrieve the heads of the entities
/// `window` selects, counting every match when `count` asks for it.
///
/// Generates filter conditions from the compiled query (IDs, types, attrs, q filter,
/// geo filter, scope query), ordering, and pagination clauses, joining only the
/// relations those clauses read.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when a reference geometry
/// (the geoquery's, or the one a distance ordering measures from) has no
/// EWKB spelling the store can bind. Returns [`RenderError::Query`] for the two
/// query-language rules a renderer is the first place to decide: `georel=near`
/// naming neither `maxDistance` nor `minDistance`, which clause 4.10 requires
/// at least one of, and an `orderBy` distance term with no reference geometry
/// anywhere in the query to measure from (clause 4.23).
pub fn build_query(
    target: &ContextTarget,
    query: &CompiledEntityQuery,
    window: &PageWindow,
    count: CountPolicy,
    names: &QueryNames,
) -> Result<EntityQuery, RenderError> {
    let schema = schema_iden(target);
    let relations = EntityRelations::for_query(query, &names.types, schema.as_ref());
    let rows = relations.rows(schema.clone(), query.selection.standing);
    let condition = build_entity_conditions(query, names, &rows, &relations.source, QFilter::Whole, GeoFilter::Matched)?;
    let shape = PageShape::of(query, names, window, count, &relations, &rows, schema.as_ref())?;
    let counted_on_page = shape.counted_on_page(count, window);
    let source = PageSource {
        relations: &relations,
        schema: schema.as_ref(),
        rows: &rows,
        query,
        window,
        names,
    };
    let page = shape.cut(&source, &condition, count, counted_on_page)?;

    let total_source = TotalSource {
        query,
        names,
        rows: &rows,
        schema: schema.as_ref(),
    };
    let total = entity_total(&total_source, count, counted_on_page, condition, page.total_candidates)?;

    Ok(EntityQuery {
        select: page.select,
        total,
        read_order: page.read_order,
        planning: page.planning,
        total_planning: page.total_planning,
    })
}

/// Builds a parameterized SQL query to retrieve only the identifiers of the
/// entities `window` selects.
///
/// Similar to [`build_query`] but selects `id` alone, used for operations that
/// only need to identify matching entities.
///
/// # Errors
///
/// Renders the same filter and ordering as [`build_query`] and so fails in the
/// same cases: [`RenderError::ReferenceGeometryEncoding`] for a reference
/// geometry with no EWKB spelling, and [`RenderError::Query`] for a `near`
/// relation naming no distance (clause 4.10) or a distance ordering with no
/// reference geometry (clause 4.23).
pub fn build_ids_query(target: &ContextTarget, query: &CompiledEntityQuery, window: &PageWindow, names: &QueryNames) -> Result<EntityIdsQuery, RenderError> {
    let schema = schema_iden(target);
    let relations = EntityRelations::for_query(query, &names.types, schema.as_ref());
    let rows = relations.rows(schema.clone(), query.selection.standing);
    let condition = build_entity_conditions(query, names, &rows, &relations.source, QFilter::Whole, GeoFilter::Matched)?;

    let mut select = Query::select();
    select.expr_as(rows.identity_column(Entities::Id), Entities::Id);
    relations.apply_from(&mut select, schema.as_ref(), query.selection.standing);
    select.cond_where(condition);

    let (keys, read_order) = window_and_sort_keys(&mut select, &rows, query, window, &names.attributes)?;
    for key in keys {
        key.order_by(&mut select);
    }

    Ok(EntityIdsQuery {
        select: select.take(),
        read_order,
    })
}

/// Add the result window to `select` and return the keys it is ordered by and
/// the order its rows come back in.
///
/// An offset window (clause 5.5.9.2) carries `LIMIT`/`OFFSET` over the
/// requested `orderBy` and its `id` tiebreaker. A cursor window (clause
/// 5.5.9.1) drops the `OFFSET` for the direction-appropriate predicate and
/// ordering over the entity `id`: the same bare, C-collation `id` expression
/// the implicit tiebreaker sorts on, which stays driven by its unique index in
/// either direction (an Index Scan Backward for the descending cases). The
/// pagination policy never pairs a cursor with a requested ordering, which a
/// cursor's identifier position cannot walk.
///
/// # Errors
///
/// Propagates the [`RenderError`] a distance `orderBy` term raises when it has
/// no reference geometry to measure from (clause 4.23); a cursor window carries
/// no `orderBy` and so never fails here.
pub(crate) fn window_and_sort_keys(
    select: &mut SelectStatement,
    rows: &EntityRowRefs,
    query: &CompiledEntityQuery,
    window: &PageWindow,
    attributes: &ResolvedNames<AttributeName>,
) -> Result<(Vec<SortKey>, ReadOrder), RenderError> {
    let identifier = rows.identity_column(Entities::Id);
    let read_order = bound_to_window(select, &identifier, window);
    let keys = match window.start {
        PageStart::Offset(_) => result_ordering::sort_keys(select, rows, &query.ordering, attributes)?,
        PageStart::Cursor(_) => vec![SortKey::new(identifier, KeyOrder::new(read_order.into(), NullPlacement::Unstated))],
    };
    Ok((keys, read_order))
}

/// How much of a query's `q` a statement filters on.
#[derive(Debug, Clone, Copy)]
pub(crate) enum QFilter<'q> {
    /// The whole expression.
    Whole,
    /// The expression without one term of its top-level conjunction, which
    /// the statement answers by other means.
    Except(&'q TermReading),
    /// None of it: the statement's rows are filtered by `q` further out.
    Omitted,
}

/// Whether a statement filters by the query's geo-query.
#[derive(Debug, Clone, Copy)]
pub(crate) enum GeoFilter {
    /// The selection is narrowed to the Entities the geo-query matches.
    Matched,
    /// Each Entity the statement reads is tested for the geo-query by its
    /// key, for a statement that looks its Entities up one by one, where the
    /// set of every match would be read whole however few they are.
    Probed,
    /// The geo-query is left out, since each Entity the statement yields is
    /// probed for it further out, or the statement reads its matches as
    /// candidates.
    Omitted,
}

/// A query's filters, split by what each does to the Entities a statement
/// reads.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntityConditions {
    /// Which Entities are read: the considered set and the selectors, tests
    /// of the Entity's own rows an index can narrow the read by.
    pub(crate) selection: Vec<Condition>,
    /// What each Entity read is tested by: `attrs`, the projected members,
    /// `q` and the geo-query as the statement asks, and `scopeQ`.
    pub(crate) tested: Vec<Condition>,
}

impl EntityConditions {
    /// Every filter in one condition, the selection's first.
    pub(crate) fn whole(self) -> Condition {
        all_of(self.selection.into_iter().chain(self.tested))
    }
}

/// Every condition of `conditions`, each a term of one conjunction.
pub(crate) fn all_of(conditions: impl IntoIterator<Item = Condition>) -> Condition {
    conditions.into_iter().fold(Condition::all(), Condition::add)
}

/// Builds sea-query `Condition` from compiled entity query filters.
///
/// The branch a statement runs against is chosen by the schema its relations
/// are qualified with, not by a predicate, so no branch term appears here; the
/// `rows` carry that schema to any subquery a filter opens, and `source` is
/// the relation the statement is driven by, which decides what of the
/// selectors is left to filter.
pub(crate) fn build_entity_conditions(
    query: &CompiledEntityQuery,
    names: &QueryNames,
    rows: &EntityRowRefs,
    source: &RowSource,
    q: QFilter<'_>,
    geo: GeoFilter,
) -> Result<Condition, RenderError> {
    Ok(entity_conditions(query, names, rows, source, q, geo)?.whole())
}

/// The filters of [`build_entity_conditions`], split into the selection and
/// what each Entity it reads is tested by.
pub(crate) fn entity_conditions(
    query: &CompiledEntityQuery,
    names: &QueryNames,
    rows: &EntityRowRefs,
    source: &RowSource,
    q: QFilter<'_>,
    geo: GeoFilter,
) -> Result<EntityConditions, RenderError> {
    Ok(EntityConditions {
        selection: selection_conditions(query, names, rows, source),
        tested: tested_conditions(query, names, rows, q, geo)?,
    })
}

/// Which Entities a statement driven by `source` reads: the considered set
/// and the selectors.
pub(crate) fn selection_conditions(query: &CompiledEntityQuery, names: &QueryNames, rows: &EntityRowRefs, source: &RowSource) -> Vec<Condition> {
    let mut selection = Vec::new();

    // Clause 5.5.14: a request narrowed to an empty identifier set considers no
    // entity, so nothing can match it whatever the selectors and filters admit.
    // Rendered as a constant `FALSE` rather than as an empty
    // `id = ANY(ARRAY[]::text[])`: the two match the same nothing, and the
    // constant is the one PostgreSQL folds into a one-time filter that reads no
    // row at all.
    match query.selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => selection.push(false_literal().into()),
    }

    if let Some(selectors) = selector_condition(&query.selection, rows, &names.types, source) {
        selection.push(selectors);
    }
    selection
}

/// What each Entity a statement reads through `rows` is tested by: `attrs`,
/// the projected members, `q` and the geo-query as `q` and `geo` say, and
/// `scopeQ`, the last read off the search row named `entity_search`.
///
/// # Errors
///
/// Fails as rendering the geo-query fails: a reference geometry with no EWKB
/// spelling, or `georel=near` naming no distance.
pub(crate) fn tested_conditions(
    query: &CompiledEntityQuery,
    names: &QueryNames,
    rows: &EntityRowRefs,
    q: QFilter<'_>,
    geo: GeoFilter,
) -> Result<Vec<Condition>, RenderError> {
    let mut tested = Vec::new();

    if let Some(attrs) = attrs_condition(&query.selection, names, rows) {
        tested.push(attrs);
    }

    // Clauses 5.7.2.5 and 6.3.13: an entity the `pick` / `omit` projection
    // would reduce to no member is not selected, so it is left out here, where
    // the count and the page are taken.
    if let Some(members) = projected_member_condition(&query.selection.projected_members, names, rows) {
        tested.push(members);
    }

    if let Some(compiled) = &query.selection.q {
        let scan = InstanceScan::Current(rows);
        match q {
            QFilter::Whole => tested.push(q_filter::compile(compiled.reading(), &scan, names)),
            QFilter::Except(answered) => tested.push(q_filter::compile_except(compiled.reading(), answered, &scan, names)),
            QFilter::Omitted => {}
        }
    }

    if let Some(geo_query) = &query.selection.geo {
        match geo {
            GeoFilter::Matched => tested.push(geo_sql::condition(geo_query, rows, &query.selection.default_location, &names.attributes)?),
            GeoFilter::Probed => tested.push(geo_sql::probe(geo_query, rows, &names.attributes)?),
            GeoFilter::Omitted => {}
        }
    }

    if let Some(scope_q) = &query.selection.scope_q {
        tested.push(scope_paths_condition(scope_q));
    }

    Ok(tested)
}

/// `scope_q` (clause 4.19) tested on the scope paths of the search row named
/// `entity_search`.
pub(crate) fn scope_paths_condition(scope_q: &ScopeQuery) -> Condition {
    scope_condition(scope_q, &ScopeTarget::paths((EntitySearch::Table, EntitySearch::ScopePaths)))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{build_ids_query, build_query},
        entity_total::EntityTotal,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        entity::compiled::CompiledEntityQuery,
        entity_constraint::EntityConstraint,
        entity_selection::{ConsideredEntities, EntitySelection},
        filter::{
            Filter,
            attribute_path::AttributePath,
            compiled_filter::CompiledFilter,
            language_matching::LanguageMatching,
            query_term::{OrderingOperator, QueryTerm},
            scalar::Scalar,
            value_path::ValuePath,
        },
        geo::compiled_geo_query::CompiledGeoQuery,
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        ordering::{
            compiled_ordering_term::CompiledOrderingTerm,
            expanded_ordering_target::ExpandedOrderingTarget,
            icu_collation::IcuCollation,
            ordering_direction::OrderingDirection,
            ordering_error::OrderingError,
            ordering_geometry::OrderingGeometry,
        },
        page_window::{PageStart, PageWindow},
        pattern::Pattern,
        projection::{expanded_projection::ExpandedProjection, expanded_projection_term::ExpandedProjectionTerm, projected_members::ProjectedMembers},
        query_error::QueryError,
        read_order::ReadOrder,
        scope::scope_query::ScopeQuery,
        type_selection::TypeSelection,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::{error::RenderError, query_names::QueryNames};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::{PostgresQueryBuilder, SelectStatement, Value};
    use std::str::FromStr;

    /// A built entity query: the paginated statement and, when the query is
    /// counted, the separate count the store runs beside it.
    struct Built {
        select: SelectStatement,
        count: Option<SelectStatement>,
    }

    /// The first offset page of 20, the window most tests read.
    fn first_offset_page() -> PageWindow {
        PageWindow::offset(20, 0)
    }

    /// Render an uncounted first offset page of an entity query.
    fn build(query: &CompiledEntityQuery, names: &QueryNames) -> Built {
        build_paged(query, &first_offset_page(), CountPolicy::Omit, names)
    }

    /// Render a counted first offset page of an entity query.
    fn build_counted(query: &CompiledEntityQuery, names: &QueryNames) -> Built {
        build_paged(query, &first_offset_page(), CountPolicy::Emit, names)
    }

    /// Build an entity query the way the store executes it: the paginated
    /// statement and, when requested, its separate count.
    fn build_paged(query: &CompiledEntityQuery, window: &PageWindow, count: CountPolicy, names: &QueryNames) -> Built {
        let target = ContextTarget::Live;
        let entity_query = build_query(&target, query, window, count, names).expect("valid entity query");
        let count = match entity_query.total {
            EntityTotal::Omitted => None,
            EntityTotal::OnPageRows { uncounted: count } | EntityTotal::Counted(count) => Some(count.count),
        };
        Built {
            select: entity_query.select,
            count,
        }
    }

    fn ids(raw: &[&str]) -> Vec<Identifier> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// The type dictionary every test resolves against: the example types the
    /// selectors below name, keyed in declaration order.
    fn known_types() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::default(),
            types: ResolvedNames::from_pairs(["Vehicle", "Home", "Motorhome", "T1", "T2"].into_iter().enumerate().map(|(index, name)| {
                let key = i32::try_from(index + 1).expect("a handful of test types");
                (
                    format!("https://example.com/{name}").parse().expect("valid type name"),
                    EntityTypeNameKey::from(key),
                )
            })),
            ..QueryNames::default()
        }
    }

    /// Attribute names alone, with no type resolved.
    fn attributes_only(attributes: ResolvedNames<AttributeName>) -> QueryNames {
        QueryNames {
            attributes,
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    /// A `GeoJSON` point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: &serde_json::Value) -> Geometry {
        serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    /// A query with every filter absent, for tests that vary one part at a time.
    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build()
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-empty-query-returns-base-sql", built.select);
        assert!(built.count.is_none());
    }

    #[test]
    fn build_empty_query_orders_by_id_for_stable_pagination() {
        // Clause 5.5.9: a paginated result needs a consistent order across
        // pages. A query with no `orderBy` must still emit the `id` tiebreaker
        // rather than leaving `LIMIT`/`OFFSET` over the plan's arbitrary order,
        // which can repeat and skip rows between pages.
        let q = empty_query();
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-empty-query-orders-by-id-for-stable-pagination", built.select);
    }

    /// A query that filters on nothing cuts its page from the identity row
    /// alone and looks each kept row's head up by key outside the window, so
    /// an `OFFSET` walks entity keys and never reads the heads it skips.
    #[test]
    fn a_query_with_no_filter_joins_nothing_and_reads_the_page_heads_by_key() {
        let q = empty_query();
        let built = build_paged(&q, &PageWindow::offset(20, 5000), CountPolicy::Omit, &known_types());
        assert_statement_snapshot!("entity-a-query-with-no-filter-joins-nothing-and-reads-the-page-heads-by-key", built.select);
    }

    /// A `q` filter probes the rows of its attribute inside the page, which
    /// joins nothing for it, and the heads are still read by key for the kept
    /// rows only.
    #[test]
    fn a_q_filter_probes_inside_the_page_and_the_heads_are_read_outside_it() {
        let mut q = empty_query();
        q.selection.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded("\"https://example.com/temperature\"!=1".parse::<Filter<Written>>().expect("valid q"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        let built = build(&q, &temperature_is_known());
        assert_statement_snapshot!("entity-a-q-filter-probes-inside-the-page-and-the-heads-are-read-outside-it", built.select);
    }

    /// Clause 4.23.1 returns the entities sorted, so every key the page is cut
    /// by is restated on the head statement in the same direction and null
    /// placement, ending with the `id` tiebreaker.
    #[test]
    fn the_head_statement_restates_every_key_of_the_page_order() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::ModifiedAt,
            direction: OrderingDirection::Desc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-the-head-statement-restates-every-key-of-the-page-order", built.select);
    }

    /// The total is counted inside the page, over every matching entity before
    /// the window cuts it, and carried out beside each head.
    #[test]
    fn a_counted_query_counts_inside_the_page_and_carries_the_total_out() {
        let q = empty_query();
        let built = build_counted(&q, &known_types());
        assert_statement_snapshot!("entity-a-counted-query-counts-inside-the-page-and-carries-the-total-out", built.select);
    }

    /// Clause 5.5.9 `attrs` keeps an Entity holding a standing instance of one
    /// of the Attributes (clause 4.22), which its current instances decide,
    /// beside the search row's interned names, one containment test per term
    /// the GIN index serves, that name its candidates; a name the dictionary
    /// never recorded is carried by no entity and matches nothing.
    #[test]
    fn attrs_terms_test_the_search_row_and_a_standing_instance() {
        let mut q = empty_query();
        q.selection.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/neverWritten")];
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(11))]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        };
        let built = build(&q, &names);
        assert_statement_snapshot!("entity-attrs-terms-test-the-search-row-and-a-standing-instance", built.select);
    }

    /// The dictionary the projection tests resolve against: `speed` at key 11
    /// and `scope` at key 12, the latter recorded as an entity member name.
    fn speed_and_scope_are_known() -> QueryNames {
        attributes_only(ResolvedNames::from_pairs([
            ("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(11)),
            (
                "https://uri.etsi.org/ngsi-ld/scope".parse().expect("valid attribute name"),
                AttributeNameKey::from(12),
            ),
        ]))
    }

    fn projected(projection: &ExpandedProjection) -> CompiledEntityQuery {
        let mut q = empty_query();
        q.selection.projected_members = ProjectedMembers::of(projection);
        q
    }

    fn speed_term() -> ExpandedProjectionTerm {
        ExpandedProjectionTerm::Attribute {
            iri: iri("https://example.com/speed"),
            linked: Vec::new(),
        }
    }

    /// Clauses 5.7.2.5 and 6.3.13: a `pick` of Attributes
    /// selects the entities carrying one of them, in the page and in the count
    /// alike; a name never recorded keeps no entity.
    #[test]
    fn a_pick_of_attributes_selects_the_entities_carrying_one_of_them() {
        let q = projected(
            &ExpandedProjection::builder()
                .pick(vec![
                    speed_term(),
                    ExpandedProjectionTerm::Attribute {
                        iri: iri("https://example.com/neverWritten"),
                        linked: Vec::new(),
                    },
                ])
                .build(),
        );
        let built = build_counted(&q, &speed_and_scope_are_known());
        assert_statement_snapshot!("entity-a-pick-of-attributes-selects-the-entities-carrying-one-of-them-1", built.select);
        let count = built.count.expect("count requested");
        assert_statement_snapshot!("entity-a-pick-of-attributes-selects-the-entities-carrying-one-of-them-2", count);
    }

    /// Clause 4.21: an `omit` of `id` and `type` keeps the entities carrying a
    /// member name outside those it and clause 5.2.4 set apart.
    #[test]
    fn an_omit_of_id_and_type_selects_the_entities_carrying_another_attribute() {
        let q = projected(
            &ExpandedProjection::builder()
                .omit(vec![
                    ExpandedProjectionTerm::Id,
                    ExpandedProjectionTerm::Type,
                    ExpandedProjectionTerm::Scope,
                    speed_term(),
                ])
                .build(),
        );
        let built = build_counted(&q, &speed_and_scope_are_known());
        assert_statement_snapshot!("entity-an-omit-of-id-and-type-selects-the-entities-carrying-another-attribute", built.select);
    }

    /// A projection that keeps `scope` keeps an entity through its scope paths.
    #[test]
    fn a_pick_of_scope_selects_the_scoped_entities() {
        let q = projected(&ExpandedProjection::builder().pick(vec![ExpandedProjectionTerm::Scope]).build());
        let built = build_counted(&q, &speed_and_scope_are_known());
        assert_statement_snapshot!("entity-a-pick-of-scope-selects-the-scoped-entities", built.select);
    }

    /// Every entity has an `id`, so a projection keeping it selects them all.
    #[test]
    fn a_pick_of_id_selects_every_entity() {
        let q = projected(&ExpandedProjection::builder().pick(vec![ExpandedProjectionTerm::Id, speed_term()]).build());
        let built = build_counted(&q, &speed_and_scope_are_known());
        assert_statement_snapshot!("entity-a-pick-of-id-selects-every-entity", built.select);
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.selection.selectors = vec![EntityConstraint::builder().ids(ids(&["urn:a", "urn:b"])).build()];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-ids", built.select);
    }

    /// Clause 5.5.14: "An `EntityMap` fixes the Entities to be considered for
    /// subsequent requests based on it." A request whose identifiers do not
    /// intersect the map it runs under is left considering no entity, and the
    /// statement must match nothing rather than fall back on the unconstrained
    /// reading an empty identifier list would otherwise have.
    #[test]
    fn a_query_that_considers_no_entity_renders_an_unsatisfiable_statement() {
        let mut q = empty_query();
        q.selection.considered = ConsideredEntities::NoEntity;
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-a-query-that-considers-no-entity-renders-an-unsatisfiable-statement", built.select);
    }

    /// The clause 5.5.14 narrowing is conjoined with the rest of the query, so
    /// the filters a map-scoped request still carries cannot widen it back.
    #[test]
    fn considering_no_entity_survives_beside_the_other_filters() {
        let mut q = empty_query();
        q.selection.considered = ConsideredEntities::NoEntity;
        q.selection.selectors = vec![EntityConstraint::builder().ids(ids(&["urn:a"])).build()];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-considering-no-entity-survives-beside-the-other-filters", built.select);
    }

    /// A request that named no identifier at all considers whatever it selects,
    /// which is the case an empty identifier list must never be confused with.
    #[test]
    fn a_query_that_named_no_identifier_renders_no_unsatisfiable_term() {
        let built = build(&empty_query(), &known_types());
        assert_statement_snapshot!("entity-a-query-that-named-no-identifier-renders-no-unsatisfiable-term", built.select);
    }

    /// A warm spatial prefilter feeds its candidate id set into the selector,
    /// and at scale that set can exceed PostgreSQL's 65535 parameter ceiling.
    /// The id membership must therefore bind as a single `text[]` (`= ANY($n)`)
    /// beside the single EWKB reference geometry, never one placeholder per id.
    #[test]
    fn a_near_query_with_a_candidate_id_set_binds_one_array_beside_the_geometry() {
        let mut q = empty_query();
        q.selection.selectors = vec![EntityConstraint::builder().ids(ids(&["urn:a", "urn:b", "urn:c"])).build()];
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let built = build_counted(&q, &known_types());
        assert_statement_snapshot!("entity-a-near-query-with-a-candidate-id-set-binds-one-array-beside-the-geometry", built.select);
        assert_statement_snapshot!(
            "entity-a-near-query-with-a-candidate-id-set-binds-one-array-beside-the-geometry-geo-page",
            build(&q, &known_types()).select
        );
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .id_pattern(Some("urn:ngsi-ld:Vehicle:.*".parse::<Pattern>().unwrap()))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-id-pattern", built.select);
    }

    /// Clauses 4.17 and 5.5.9: a page of one type walks that type's membership
    /// key range in identifier order, so it reads no Entity of another type,
    /// and the identifier it sorts and cuts on is the walk's.
    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(iri("https://example.com/Vehicle").into())))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-single-type", built.select);
    }

    /// A type the dictionary has never recorded was never carried by any
    /// entity, so a selector on it walks an empty range.
    #[test]
    fn an_unknown_type_selector_matches_nothing() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(iri("https://example.com/NeverWritten").into())))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-an-unknown-type-selector-matches-nothing", built.select);
    }

    /// A counted page of one type counts the walk it pages.
    #[test]
    fn the_count_of_a_type_page_counts_the_walk() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(iri("https://example.com/Vehicle").into())))
                .build(),
        ];
        let count = build_counted(&q, &known_types()).count.expect("count requested");
        assert_statement_snapshot!("entity-the-count-of-a-type-page-counts-the-walk", count);
    }

    /// Clause 5.2.33: a selector naming identifiers is answered by the
    /// identifier index, so its type stays a filter on the search row.
    #[test]
    fn a_typed_selector_naming_identifiers_filters_the_search_row() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .ids(ids(&["urn:a"]))
                .type_selection(Some(TypeSelection::single(iri("https://example.com/Vehicle").into())))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-a-typed-selector-naming-identifiers-filters-the-search-row", built.select);
    }

    /// A geo-query's matches drive the statement and its count, so the types
    /// of a typed geo-query are tested on the search row and no membership
    /// range is merged.
    #[test]
    fn a_typed_geo_query_tests_its_types_on_the_search_row() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(TypeSelection::of_alternatives(vec![
                    vec![iri("https://example.com/Vehicle").into()],
                    vec![iri("https://example.com/Home").into()],
                ]))
                .build(),
        ];
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let built = build_counted(&q, &known_types());
        assert_statement_snapshot!("entity-a-typed-geo-query-tests-its-types-on-the-search-row-1", built.select);
        let count = built.count.expect("count requested");
        assert_statement_snapshot!("entity-a-typed-geo-query-tests-its-types-on-the-search-row-2", count);
    }

    #[test]
    fn build_with_mixed_type_expr() {
        let mut q = empty_query();
        // (Home AND Vehicle) OR Motorhome
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(TypeSelection::of_alternatives(vec![
                    vec![iri("https://example.com/Home").into(), iri("https://example.com/Vehicle").into()],
                    vec![iri("https://example.com/Motorhome").into()],
                ]))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-mixed-type-expr", built.select);
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(iri("https://example.com/Vehicle").into())))
                .build(),
        ];
        let built = build_counted(&q, &known_types());
        let count = built.count.expect("count requested");
        assert_statement_snapshot!("entity-build-with-count", count);
    }

    #[test]
    fn a_union_of_selectors_ors_them_rather_than_cross_producting() {
        // Clause 5.2.33: a POST `entities[]` body is a union of independent
        // selectors, so `{id:A,type:T1}` and `{id:B,type:T2}` render
        // `(A AND T1) OR (B AND T2)` and never the cross product that would also
        // match `(A,T2)`.
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntityConstraint::builder()
                .ids(ids(&["urn:ngsi-ld:Entity:A"]))
                .type_selection(Some(TypeSelection::single(iri("https://example.com/T1").into())))
                .build(),
            EntityConstraint::builder()
                .ids(ids(&["urn:ngsi-ld:Entity:B"]))
                .type_selection(Some(TypeSelection::single(iri("https://example.com/T2").into())))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-a-union-of-selectors-ors-them-rather-than-cross-producting", built.select);
    }

    fn temperature_is_known() -> QueryNames {
        attributes_only(ResolvedNames::from_pairs([(
            "https://example.com/temperature".parse().expect("valid attribute name"),
            AttributeNameKey::from(12),
        )]))
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.selection.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded("\"https://example.com/temperature\"".parse::<Filter<Written>>().unwrap())
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        let built = build(&q, &temperature_is_known());
        // Row presence is existence: a deleted attribute leaves no row.
        assert_statement_snapshot!("entity-build-with-q-existence", built.select);
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        cq.selection.q = Some(CompiledFilter::new(
            Filter::Term(QueryTerm::Ordering {
                path: AttributePath::Local(ValuePath::simple("https://example.com/temperature".parse().expect("valid attribute name"))),
                operator: OrderingOperator::Greater,
                value: Scalar::Number(50.into()),
            }),
            LanguageMatching::TagEquality,
        ));
        let built = build(&cq, &temperature_is_known());
        assert_statement_snapshot!("entity-build-with-q-comparison", built.select);
    }

    #[test]
    fn count_sql_reuses_the_main_query_bind_vector() {
        // The repository binds one vector against both statements, so the
        // placeholder sequence has to be identical up to the pagination
        // parameters the count query omits. A String ordering keeps the page
        // plain: no index orders it.
        let mut cq = empty_query();
        cq.selection.q = Some(CompiledFilter::new(
            Filter::Term(QueryTerm::Ordering {
                path: AttributePath::Local(ValuePath::simple("https://example.com/temperature".parse().expect("valid attribute name"))),
                operator: OrderingOperator::Greater,
                value: Scalar::Str("m".to_owned()),
            }),
            LanguageMatching::TagEquality,
        ));
        let built = build_counted(&cq, &temperature_is_known());
        assert_statement_snapshot!("entity-count-sql-reuses-the-main-query-bind-vector-1", built.select);
        let count = built.count.expect("count sql");
        assert_statement_snapshot!("entity-count-sql-reuses-the-main-query-bind-vector-2", count);
        let placeholders = |sql: &str| {
            let mut found: Vec<String> = Vec::new();
            let bytes = sql.as_bytes();
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'$' {
                    let digits: String = sql[index + 1..].chars().take_while(char::is_ascii_digit).collect();
                    if !digits.is_empty() {
                        found.push(digits);
                    }
                }
            }
            found
        };
        let main = placeholders(&built.select.build(PostgresQueryBuilder).0);
        let counted = placeholders(&count.build(PostgresQueryBuilder).0);
        assert_eq!(counted, main[..counted.len()], "count query must reuse the leading binds verbatim");
    }

    #[test]
    fn build_with_limit_offset() {
        let q = empty_query();
        let built = build_paged(&q, &PageWindow::offset(50, 100), CountPolicy::Omit, &known_types());
        assert_statement_snapshot!("entity-build-with-limit-offset", built.select);
    }

    fn boundary(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    /// A cursor page of 20 starting at `cursor`.
    fn cursor_page(cursor: PageCursor) -> PageWindow {
        PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(cursor)),
        }
    }

    /// Clause 5.5.9.1: a forward cursor pages past the boundary identifier over
    /// the default `id` order (`WHERE id > $x ORDER BY id ASC LIMIT n`), with no
    /// `OFFSET`, because the keyset predicate replaces the skip.
    #[test]
    fn build_with_keyset_after_pages_forward_ascending_without_offset() {
        let q = empty_query();
        let built = build_paged(
            &q,
            &PageWindow::after(20, boundary("urn:ngsi-ld:Building:001")),
            CountPolicy::Omit,
            &known_types(),
        );
        assert_statement_snapshot!("entity-build-with-keyset-after-pages-forward-ascending-without-offset", built.select);
    }

    /// Clause 5.5.9.1: a backward cursor pages before the boundary over the
    /// reversed order (`WHERE id < $x ORDER BY id DESC LIMIT n`), which the
    /// store reverses to ascending after fetching.
    #[test]
    fn build_with_keyset_before_pages_backward_descending_without_offset() {
        let q = empty_query();
        let built = build_paged(
            &q,
            &cursor_page(PageCursor::Before(boundary("urn:ngsi-ld:Building:042"))),
            CountPolicy::Omit,
            &known_types(),
        );
        assert_statement_snapshot!("entity-build-with-keyset-before-pages-backward-descending-without-offset", built.select);
    }

    /// Clause 5.5.9.1: the from-end cursor takes the final page (`ORDER BY id
    /// DESC LIMIT n`, no boundary predicate), which the store reverses to
    /// ascending. This is the Andromeda `last` anchor.
    #[test]
    fn build_with_keyset_end_takes_the_final_page_descending() {
        let q = empty_query();
        let built = build_paged(&q, &cursor_page(PageCursor::End), CountPolicy::Omit, &known_types());
        assert_statement_snapshot!("entity-build-with-keyset-end-takes-the-final-page-descending", built.select);
    }

    /// Clause 6.3.13: a counted keyset page counts the whole selection with a
    /// separate statement. A window count over the page would see only the
    /// entities past the cursor, so the page carries none, and the count
    /// statement carries no boundary predicate.
    #[test]
    fn a_counted_keyset_page_counts_the_whole_selection_separately() {
        let q = empty_query();
        let target = ContextTarget::Live;
        let window = PageWindow::after(20, boundary("urn:ngsi-ld:Building:001"));
        let entity_query = build_query(&target, &q, &window, CountPolicy::Emit, &known_types()).expect("valid entity query");
        assert_statement_snapshot!("entity-a-counted-keyset-page-counts-the-whole-selection-separately-1", entity_query.select);
        let count = match entity_query.total {
            EntityTotal::Counted(count) => Some(count),
            EntityTotal::Omitted | EntityTotal::OnPageRows { .. } => None,
        }
        .expect("a counted keyset page is counted separately");
        assert_statement_snapshot!("entity-a-counted-keyset-page-counts-the-whole-selection-separately-2", count.count);
    }

    /// A counted offset page keeps carrying its total on the page rows, with
    /// the separate count kept for an empty page.
    #[test]
    fn a_counted_offset_page_carries_its_total_on_the_page_rows() {
        let q = empty_query();
        let target = ContextTarget::Live;
        let entity_query = build_query(&target, &q, &first_offset_page(), CountPolicy::Emit, &known_types()).expect("valid entity query");
        assert_statement_snapshot!("entity-a-counted-offset-page-carries-its-total-on-the-page-rows", entity_query.select);
        assert!(matches!(entity_query.total, EntityTotal::OnPageRows { .. }));
    }

    /// The ids-only statement pages by keyset the same way the head one does.
    #[test]
    fn build_ids_query_pages_by_keyset() {
        let q = empty_query();
        let target = ContextTarget::Live;
        let window = PageWindow::after(20, boundary("urn:ngsi-ld:Building:001"));
        let ids_query = build_ids_query(&target, &q, &window, &known_types()).expect("valid ids query");
        assert_statement_snapshot!("entity-build-ids-query-pages-by-keyset", ids_query.select);
    }

    /// Clause 4.23.1 sorts by the attribute's value, which the statement reads
    /// from the attribute's first row, and clause 4.23.2 ranks that value's
    /// datatype before comparing it.
    #[test]
    fn build_with_order_by_asc() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple("https://example.com/name".parse().expect("valid attribute name"))),
            direction: OrderingDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-order-by-asc", built.select);
    }

    #[test]
    fn build_with_order_by_collation() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple("https://example.com/name".parse().expect("valid attribute name"))),
            direction: OrderingDirection::Asc,
            collation: Some("und-u-ks-identic".parse::<IcuCollation>().expect("valid collation")),
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-order-by-collation", built.select);
    }

    /// Clause 4.8 with clause 4.23: `orderBy=createdAt` sorts on the identity
    /// row's own `created_at` column.
    #[test]
    fn build_with_order_by_created_at_reads_the_identity_column() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::CreatedAt,
            direction: OrderingDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-order-by-created-at-reads-the-identity-column", built.select);
    }

    /// A descending `createdAt` reverses only the order.
    #[test]
    fn build_with_order_by_created_at_desc_reverses_the_order() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::CreatedAt,
            direction: OrderingDirection::Desc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-order-by-created-at-desc-reverses-the-order", built.select);
    }

    /// Clause 4.8 `modifiedAt` is a column of the identity row that is never
    /// null, so the term joins nothing and states no null placement.
    #[test]
    fn build_with_order_by_modified_at_reads_the_identity_column() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::ModifiedAt,
            direction: OrderingDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-order-by-modified-at-reads-the-identity-column", built.select);
    }

    /// Clause 4.23.2 ranks "Attribute does not exist" last, and an unscoped
    /// entity stores no scope path array.
    #[test]
    fn ordering_by_scope_ranks_an_unscoped_entity_last() {
        for (index, direction) in [OrderingDirection::Asc, OrderingDirection::Desc].into_iter().enumerate() {
            let mut q = empty_query();
            q.ordering.terms = vec![CompiledOrderingTerm {
                target: ExpandedOrderingTarget::Scope,
                direction,
                collation: None,
            }];
            let built = build(&q, &known_types());
            assert_statement_snapshot!(format!("entity-ordering-by-scope-ranks-an-unscoped-entity-last-{}", index + 1), built.select);
        }
    }

    /// Table 6.4.3.2-1 makes `orderFrom` mandatory for a distance ordering, so
    /// the geo-query's geometry is not measured from in its place.
    #[test]
    fn a_distance_ordering_beside_a_geo_query_without_a_reference_geometry_is_refused() {
        let mut q = empty_query();
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderingDirection::DistAsc,
            collation: None,
        }];
        let target = ContextTarget::Live;
        let result = build_query(&target, &q, &first_offset_page(), CountPolicy::Omit, &known_types());
        assert!(matches!(
            result,
            Err(RenderError::Query(QueryError::Ordering(OrderingError::DistanceWithoutReference)))
        ));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_returns_error() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderingDirection::DistAsc,
            collation: None,
        }];
        let target = ContextTarget::Live;
        let result = build_query(&target, &q, &first_offset_page(), CountPolicy::Omit, &known_types());
        assert!(result.is_err());
    }

    #[test]
    fn build_with_order_by_dist_with_ordering_geometry() {
        let mut q = empty_query();
        let geometry = serde_json::from_value::<Geometry>(serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderingDirection::DistAsc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-build-with-order-by-dist-with-ordering-geometry", built.select);
    }

    #[test]
    fn build_with_order_by_dist_ordering_geometry_takes_precedence() {
        let mut q = empty_query();
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([2.3522, 48.8566])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let geometry = serde_json::from_value::<Geometry>(serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderingDirection::DistAsc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        // Ordering geometry (Berlin) takes precedence over the geo query (Paris).
        assert_statement_snapshot!("entity-build-with-order-by-dist-ordering-geometry-takes-precedence", built.select);
    }

    /// Clause 4.23.1 orders on distance over the sphere (Haversine), and the
    /// spherical measurement ignores any third ordinate. An `orderGeometry`
    /// that carries an altitude must therefore render exactly the statement a
    /// flat one renders; only the bound reference differs, because the bind
    /// keeps the altitude it was given.
    #[test]
    fn a_three_dimensional_ordering_geometry_renders_the_same_distance_sql() {
        let ordering_term = CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderingDirection::DistAsc,
            collation: None,
        };

        let mut flat_query = empty_query();
        flat_query.ordering.geometry = Some(OrderingGeometry::new(point(&serde_json::json!([13.3986, 52.5547]))));
        flat_query.ordering.terms = vec![ordering_term.clone()];
        let flat = build(&flat_query, &known_types());

        let mut elevated_query = empty_query();
        elevated_query.ordering.geometry = Some(OrderingGeometry::new(point(&serde_json::json!([13.3986, 52.5547, 34.5]))));
        elevated_query.ordering.terms = vec![ordering_term];
        let elevated = build(&elevated_query, &known_types());

        assert_statement_snapshot!("entity-a-three-dimensional-ordering-geometry-renders-the-same-distance-sql-1", flat.select);
        assert_statement_snapshot!("entity-a-three-dimensional-ordering-geometry-renders-the-same-distance-sql-2", elevated.select);
        let (flat_sql, flat_values) = flat.select.build(PostgresQueryBuilder);
        let (elevated_sql, elevated_values) = elevated.select.build(PostgresQueryBuilder);
        assert_eq!(elevated_sql, flat_sql, "distance ordering must not vary with the reference dimensionality");

        let bytes = |values: &[Value]| -> Vec<Vec<u8>> {
            values
                .iter()
                .filter_map(|value| match value {
                    Value::Bytes(Some(bytes)) => Some(bytes.clone()), // the assertion outlives the borrow of the built query
                    _non_geometry @ (Value::Bool(_)
                    | Value::TinyInt(_)
                    | Value::SmallInt(_)
                    | Value::Int(_)
                    | Value::BigInt(_)
                    | Value::TinyUnsigned(_)
                    | Value::SmallUnsigned(_)
                    | Value::Unsigned(_)
                    | Value::BigUnsigned(_)
                    | Value::Float(_)
                    | Value::Double(_)
                    | Value::BigDecimal(_)
                    | Value::String(_)
                    | Value::Enum(_)
                    | Value::Char(_)
                    | Value::Bytes(_)
                    | Value::ChronoDate(_)
                    | Value::ChronoTime(_)
                    | Value::ChronoDateTime(_)
                    | Value::ChronoDateTimeUtc(_)
                    | Value::ChronoDateTimeLocal(_)
                    | Value::ChronoDateTimeWithTimeZone(_)
                    | Value::Uuid(_)
                    | Value::Array(..)) => None,
                })
                .collect()
        };
        assert_ne!(
            bytes(&elevated_values.0),
            bytes(&flat_values.0),
            "the reference must still bind the altitude it was given"
        );
    }

    #[test]
    fn geo_default_location_includes_expiration_guard() {
        let mut q = empty_query();
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("5000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let built = build_counted(&q, &known_types());
        assert_statement_snapshot!("entity-geo-default-location-includes-expiration-guard", built.select);
        assert_statement_snapshot!(
            "entity-geo-default-location-includes-expiration-guard-geo-page",
            build(&q, &known_types()).select
        );
    }

    #[test]
    fn georel_variants_bind_reference_geometry_as_ewkb() {
        // Each GeoRel variant must parameterise the reference geometry as
        // EWKB bytes; raw GeoJSON-in-SQL is disallowed in reference position.
        let variants = vec![
            GeoRel::Within,
            GeoRel::Contains,
            GeoRel::Intersects,
            GeoRel::Disjoint,
            GeoRel::Equals,
            GeoRel::Overlaps,
            GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: Some(PositiveDistance::from_str("10").expect("positive distance")),
            },
        ];

        for (index, georel) in variants.into_iter().enumerate() {
            let mut q = empty_query();
            q.selection.geo = Some(CompiledGeoQuery {
                georel,
                geometry: point(&serde_json::json!([1.0, 2.0])),
                geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
            });
            let built = build_counted(&q, &known_types());
            assert_statement_snapshot!(format!("entity-georel-variants-bind-reference-geometry-as-ewkb-{}", index + 1), built.select);
            assert_statement_snapshot!(
                format!("entity-georel-variants-bind-reference-geometry-as-ewkb-geo-page-{}", index + 1),
                build(&q, &known_types()).select
            );
        }
    }

    /// Clause 4.10: `geoproperty` selects any `GeoProperty` of the entity. A
    /// named one is held in `current_attributes`, so the whole statement,
    /// not just the fragment, has to reach it there.
    #[test]
    fn a_named_geoproperty_filters_through_its_attribute_instance_rows() {
        let mut q = empty_query();
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/customGeo".parse().expect("valid attribute name"),
        });
        let names = attributes_only(ResolvedNames::from_pairs([(
            "https://example.com/customGeo".parse().expect("valid attribute name"),
            AttributeNameKey::from(42_i32),
        )]));
        let built = build_counted(&q, &names);
        assert_statement_snapshot!("entity-a-named-geoproperty-filters-through-its-attribute-instance-rows", built.select);
        assert_statement_snapshot!(
            "entity-a-named-geoproperty-filters-through-its-attribute-instance-rows-geo-page",
            build(&q, &names).select
        );
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching, so an unrecorded `GeoProperty` yields an empty result rather
    /// than an error.
    #[test]
    fn an_unrecorded_geoproperty_filters_everything_out_without_failing() {
        let mut q = empty_query();
        q.selection.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/neverRecorded".parse().expect("valid attribute name"),
        });
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-an-unrecorded-geoproperty-filters-everything-out-without-failing", built.select);
    }

    #[test]
    fn scope_complex_generates_mixed() {
        let mut q = empty_query();
        q.selection.scope_q = Some("(/A;/B),/C/#".parse::<ScopeQuery>().expect("valid scope query"));
        let built = build(&q, &known_types());
        assert_statement_snapshot!("entity-scope-complex-generates-mixed", built.select);
    }

    /// A geo-query names its target `GeoProperty` by IRI, but the statement
    /// reaches it by dictionary key, so no part of the IRI is interpolated and
    /// an apostrophe in one cannot terminate a literal.
    #[test]
    fn a_geoproperty_iri_containing_an_apostrophe_never_reaches_the_statement_text() {
        let names = attributes_only(ResolvedNames::from_pairs([(
            "https://example.com/o'brien".parse().expect("valid attribute name"),
            AttributeNameKey::from(9_i32),
        )]));
        let relations = [
            GeoRel::Within,
            GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("500").expect("positive distance")),
                min_distance: None,
            },
        ];
        for (index, georel) in relations.into_iter().enumerate() {
            let mut q = empty_query();
            q.selection.geo = Some(CompiledGeoQuery {
                georel,
                geometry: point(&serde_json::json!([1.0, 2.0])),
                geoproperty: "https://example.com/o'brien".parse().expect("valid attribute name"),
            });
            let built = build_counted(&q, &names);
            assert_statement_snapshot!(
                format!(
                    "entity-a-geoproperty-iri-containing-an-apostrophe-never-reaches-the-statement-text-{}",
                    index + 1
                ),
                built.select
            );
            assert_statement_snapshot!(
                format!(
                    "entity-a-geoproperty-iri-containing-an-apostrophe-never-reaches-the-statement-text-geo-page-{}",
                    index + 1
                ),
                build(&q, &names).select
            );
        }
    }

    #[test]
    fn an_order_by_attribute_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple("https://example.com/o'brien".parse().expect("valid attribute name"))),
            direction: OrderingDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert_statement_snapshot!(
            "entity-an-order-by-attribute-iri-containing-an-apostrophe-does-not-terminate-the-sql-literal",
            built.select
        );
    }

    /// A distance ordering measures through the dictionary key, and so does
    /// the clause 4.23.2 value comparison that ranks the entities whose
    /// attribute is not a `GeoProperty`, so no IRI reaches the statement.
    #[test]
    fn a_distance_ordering_reads_the_geoproperty_by_key_alone() {
        let mut q = empty_query();
        let geometry = serde_json::from_value::<Geometry>(serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Attribute(ValuePath::simple("https://example.com/o'brien".parse().expect("valid attribute name"))),
            direction: OrderingDirection::DistAsc,
            collation: None,
        }];
        let names = attributes_only(ResolvedNames::from_pairs([(
            "https://example.com/o'brien".parse().expect("valid attribute name"),
            AttributeNameKey::from(9_i32),
        )]));
        let built = build(&q, &names);
        assert_statement_snapshot!("entity-a-distance-ordering-reads-the-geoproperty-by-key-alone", built.select);
    }

    /// Clause 4.23: a requested ordering keeps its own sort keys, the `id`
    /// tiebreaker last, under an offset window.
    #[test]
    fn requested_ordering_keeps_its_sort_keys_under_an_offset_window() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderingTerm {
            target: ExpandedOrderingTarget::Id,
            direction: OrderingDirection::Desc,
            collation: None,
        }];
        let built = build_paged(&q, &PageWindow::offset(10, 30), CountPolicy::Omit, &known_types());
        assert_statement_snapshot!("entity-requested-ordering-keeps-its-sort-keys-under-an-offset-window", built.select);
    }

    /// The first page of a counted cursor walk reads every match, so its total
    /// is still carried on the page rows.
    #[test]
    fn a_counted_fresh_cursor_page_carries_its_total_on_the_page_rows() {
        let q = empty_query();
        let window = PageWindow {
            limit: 20,
            start: PageStart::Cursor(None),
        };
        let entity_query = build_query(&ContextTarget::Live, &q, &window, CountPolicy::Emit, &known_types()).expect("valid entity query");
        assert_statement_snapshot!("entity-a-counted-fresh-cursor-page-carries-its-total-on-the-page-rows", entity_query.select);
        assert!(matches!(entity_query.total, EntityTotal::OnPageRows { .. }));
        assert_eq!(entity_query.read_order, ReadOrder::Ascending);
    }
}
