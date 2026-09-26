//! SQL generation for entity queries using sea-query.

use crate::{entity_relations::EntityRelations, result_ordering};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    entity::compiled::CompiledEntityQuery,
    entity_selection::{ConsideredEntities, EntitySelector},
    pagination::CountPolicy,
};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::GeometrySource,
    q_predicate::projection as q_filter,
    scope_predicate::{ScopeTarget, condition as scope_condition},
    type_predicate::condition as type_condition,
};
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::{
    aggregate::count_over_all,
    literal::false_literal,
    operator::{array_contains_key, regex_match},
    pg_array::{any, text_array},
};
use sea_query::{Alias, Asterisk, Condition, Expr as SeaExpr, ExprTrait, Func, Order, Query, SelectStatement};

/// A compiled entity query: the paginated result statement and its optional
/// fallback count, each a statement tree the store binds and executes on its own.
///
/// The result piggybacks its total on every row as `COUNT(*) OVER ()`, but a
/// zero-row result carries no row to read it from, so the count is a separate
/// statement rather than a shared placeholder sequence - which a paginated
/// result and an unpaginated count could not be anyway.
pub struct EntityQuery {
    /// The paginated statement selecting the matching entity payloads.
    pub select: SelectStatement,
    /// The fallback `COUNT`, present when clause 5.5.9 asks for a total.
    pub count: Option<SelectStatement>,
}

/// Builds a parameterized SQL query to retrieve entity payloads.
///
/// Generates filter conditions from the compiled query (IDs, types, attrs, q expression,
/// geo filter, scope query), ordering, and pagination clauses, joining only the
/// relations those clauses read.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when a reference geometry
/// — the geoquery's, or the one a distance ordering measures from — has no
/// EWKB spelling the store can bind. Returns [`RenderError::Query`] for the two
/// query-language rules a renderer is the first place to decide: `georel=near`
/// naming neither `maxDistance` nor `minDistance`, which clause 4.10 requires
/// at least one of, and an `orderBy` distance term with no reference geometry
/// anywhere in the query to measure from (clause 4.23).
pub fn build_query(target: &ContextTarget, query: &CompiledEntityQuery, names: &QueryNames) -> Result<EntityQuery, RenderError> {
    let schema = schema_iden(target);
    let relations = EntityRelations::for_query(query);
    let rows = relations.rows(schema.clone());
    let condition = build_entity_conditions(query, names, &rows)?;

    let mut select = Query::select();
    // The payload is projected under the document column's own name whether it
    // is read from the joined row or looked up by key, so the repository reads
    // one column name in both shapes.
    select.expr_as(rows.payload_column(), EntityDocuments::Payload);
    relations.apply_from(&mut select, schema.as_ref());
    // clone: the fallback count statement filters on the same condition. The
    // count joins the document row only when a `q` filter reads it, and that
    // is the only way the condition can reference the payload, so a condition
    // rendered against this statement's rows names nothing the count lacks.
    select.cond_where(condition.clone());

    // When a total count is requested, piggyback it on every row as
    // `COUNT(*) OVER ()` so the common case runs in a single round-trip.
    // The count column is constant across the result set (PostgreSQL evaluates
    // the unbounded partition once), so the repository reads it from any row.
    // A zero-row result still needs the fallback count because the window
    // function returns no rows when the filter matches nothing.
    if query.pagination.count == CountPolicy::Emit {
        select.expr_as(count_over_all(), Alias::new(TOTAL_COUNT_COLUMN));
    }

    apply_window_and_order(&mut select, &rows, query, &names.attributes)?;

    // Fallback count query covers the zero-row edge case described above.
    let count = if query.pagination.count == CountPolicy::Emit {
        let mut count_select = Query::select();
        count_select.expr(Func::count(SeaExpr::col(Asterisk)));
        EntityRelations::for_count(query).apply_from(&mut count_select, schema.as_ref());
        count_select.cond_where(condition);
        Some(count_select.take())
    } else {
        None
    };

    Ok(EntityQuery { select: select.take(), count })
}

/// Column name used for the window-function count attached to each row of a
/// paginated entity query when `count=true`.
///
/// The entity repository reads the count back out under this name, so the two
/// have to agree; it is a constant rather than a literal for exactly that
/// reason.
pub const TOTAL_COUNT_COLUMN: &str = "total_count";

/// Builds a parameterized SQL query to retrieve only entity IDs.
///
/// Similar to [`build_query`] but selects `id` instead of `payload`, used for
/// operations that only need to identify matching entities; the document row
/// is joined only when a filter or ordering reads the payload.
///
/// # Errors
///
/// Renders the same filter and ordering as [`build_query`] and so fails in the
/// same cases: [`RenderError::ReferenceGeometryEncoding`] for a reference
/// geometry with no EWKB spelling, and [`RenderError::Query`] for a `near`
/// relation naming no distance (clause 4.10) or a distance ordering with no
/// reference geometry (clause 4.23).
pub fn build_ids_query(target: &ContextTarget, query: &CompiledEntityQuery, names: &QueryNames) -> Result<SelectStatement, RenderError> {
    let schema = schema_iden(target);
    let relations = EntityRelations::for_query(query);
    let rows = relations.rows(schema.clone());
    let condition = build_entity_conditions(query, names, &rows)?;

    let mut select = Query::select();
    select.column((Entities::Table, Entities::Id));
    relations.apply_from(&mut select, schema.as_ref());
    select.cond_where(condition);

    apply_window_and_order(&mut select, &rows, query, &names.attributes)?;

    Ok(select.take())
}

/// Add the result window and its ordering, in offset mode or keyset mode.
///
/// In offset mode (clause 5.5.9.2) the statement carries `LIMIT`/`OFFSET` over
/// the requested `orderBy` and its `id` tiebreaker. In keyset mode
/// (clause 5.5.9.1) the `OFFSET` is dropped for the direction-appropriate
/// predicate and ordering over the entity `id`: the same bare, C-collation `id`
/// expression the implicit tiebreaker sorts on, which is why keyset mode is
/// restricted to the default order. A backward or from-end page is ordered
/// descending and reversed to ascending where the store reads the rows back.
///
/// # Errors
///
/// Propagates the [`RenderError`] a distance `orderBy` term raises when it has
/// no reference geometry to measure from (clause 4.23); keyset mode carries no
/// `orderBy` and so never fails here.
fn apply_window_and_order(
    select: &mut SelectStatement,
    rows: &EntityRowRefs,
    query: &CompiledEntityQuery,
    attributes: &ResolvedNames<AttributeName>,
) -> Result<(), RenderError> {
    match &query.keyset {
        None => {
            select.limit(u64::from(query.pagination.limit)).offset(u64::from(query.pagination.offset));
            result_ordering::apply(select, rows, &query.ordering, query.selection.geo.as_ref(), attributes)?;
        }
        Some(cursor) => {
            apply_keyset(select, cursor);
            select.limit(u64::from(query.pagination.limit));
        }
    }
    Ok(())
}

/// Add the keyset predicate and ordering for one cursor direction (clause
/// 5.5.9.1).
///
/// The predicate and the ordering both read `entities.id` bare, so they share
/// the column's C collation with the implicit `id` tiebreaker and stay driven by
/// its unique index in either direction -- an Index Scan Backward for the
/// descending cases, no sort node and no new index.
fn apply_keyset(select: &mut SelectStatement, cursor: &PageCursor) {
    match cursor {
        PageCursor::After(id) => {
            select.and_where(SeaExpr::col((Entities::Table, Entities::Id)).gt(SeaExpr::val(id.as_str())));
            select.order_by((Entities::Table, Entities::Id), Order::Asc);
        }
        PageCursor::Before(id) => {
            select.and_where(SeaExpr::col((Entities::Table, Entities::Id)).lt(SeaExpr::val(id.as_str())));
            select.order_by((Entities::Table, Entities::Id), Order::Desc);
        }
        PageCursor::End => {
            select.order_by((Entities::Table, Entities::Id), Order::Desc);
        }
    }
}

/// Builds sea-query `Condition` from compiled entity query filters.
///
/// The branch a statement runs against is chosen by the schema its relations
/// are qualified with, not by a predicate, so no branch term appears here; the
/// `rows` carry that schema to any subquery a filter opens.
fn build_entity_conditions(query: &CompiledEntityQuery, names: &QueryNames, rows: &EntityRowRefs) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Clause 5.5.14: a request narrowed to an empty identifier set considers no
    // entity, so nothing can match it whatever the selectors and filters admit.
    // Rendered as a constant `FALSE` rather than as an empty
    // `id = ANY(ARRAY[]::text[])`: the two match the same nothing, and the
    // constant is the one PostgreSQL folds into a one-time filter that reads no
    // row at all.
    match query.selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => condition = condition.add(false_literal()),
    }

    // Entity selectors, unioned (clause 5.2.33): `(sel1) OR (sel2) OR ...`,
    // where each selector ANDs its own id / idPattern / type. A union that
    // contains an unconstrained selector places no constraint, so it is
    // rendered only when every selector constrains — the reading
    // `EntitySelection::selectors_constrain` shares with the in-memory residual.
    if query.selection.selectors_constrain() {
        let mut union = Condition::any();
        for selector in &query.selection.selectors {
            union = union.add(selector_condition(rows, selector, &names.types));
        }
        condition = condition.add(union);
    }

    // Attribute existence filter (clause 5.5.9 `attrs`): the entity carries a
    // member of at least one of the named attributes. The names the payload
    // carries are recorded as dictionary keys on the search row, so each term
    // is a containment test the GIN index answers; a name the dictionary has
    // never recorded is carried by no entity and its term is constant `FALSE`.
    if !query.selection.attrs.is_empty() {
        let mut attr_cond = Condition::any();
        for attr in &query.selection.attrs {
            attr_cond = attr_cond.add(match names.attributes.get_str(attr.as_str()) {
                Some(id) => array_contains_key(rows.search_column(EntitySearch::AttributeNameKeys), id.get()),
                None => false_literal(),
            });
        }
        condition = condition.add(attr_cond);
    }

    if let Some(q_expr) = &query.selection.q_expr {
        condition = condition.add(q_filter::compile(q_expr, rows, names));
    }

    if let Some(geo) = &query.selection.geo {
        condition = condition.add(geo_sql::condition(geo, GeometrySource::Entity { rows }, &names.attributes)?);
    }

    if let Some(scope_q) = &query.selection.scope_q {
        condition = condition.add(scope_condition(
            scope_q,
            &ScopeTarget::current_paths((EntitySearch::Table, EntitySearch::ScopePaths)),
        ));
    }

    Ok(condition)
}

/// Render one selector's identifier, pattern and type predicates, `ANDed`.
///
/// The three axes of a single [`EntitySelector`] hold together (clause 5.2.33):
/// `{id:A,type:T}` selects entity `A` *and only if* it is a `T`. A selector
/// with no axis set contributes an empty conjunction, which sea-query renders
/// as always-true — the unconstrained case the caller filters out before it
/// gets here.
fn selector_condition(rows: &EntityRowRefs, selector: &EntitySelector, types: &ResolvedNames<EntityTypeName>) -> Condition {
    let mut condition = Condition::all();
    if !selector.ids.is_empty() {
        let ids: Vec<&str> = selector.ids.iter().map(AsRef::as_ref).collect();
        // A warm spatial prefilter can push tens of thousands of candidate ids
        // here; `IN ($1, … $N)` would bind one parameter per id and blow past
        // PostgreSQL's 65535 parameter ceiling. A single bound `text[]` with
        // `= ANY($1)` keeps it to one parameter regardless of set size.
        condition = condition.add(rows.identity_column(Entities::Id).eq(any(text_array(&ids))));
    }
    if let Some(pattern) = &selector.id_pattern {
        condition = condition.add(regex_match(rows.identity_column(Entities::Id), SeaExpr::val(pattern.as_str())));
    }
    if let Some(type_expr) = &selector.type_expr {
        condition = condition.add(type_condition(type_expr, &rows.search_column(EntitySearch::EntityTypeNameKeys), types));
    }
    condition
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        entity_selection::{ConsideredEntities, EntitySelection, EntitySelector},
        filter::{
            expression::Expr,
            parser,
            path::{AttrPath, QueryPath},
            predicate::{OrderingOp, Predicate},
            scalar::Scalar,
        },
        geo::query::GeoQuery,
        ordering::{
            collation::IcuCollation,
            geometry::OrderingGeometry,
            term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        },
        pagination::CountPolicy,
        pattern::Pattern,
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
    use iri_rs::IriBuf;
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::{PostgresQueryBuilder, Value};
    use std::str::FromStr;

    /// A built entity query rendered with `$N` placeholders and its bound
    /// values, which is what the store hands the driver.
    struct Built {
        sql: String,
        count_sql: Option<String>,
        bind_values: Vec<Value>,
    }

    /// Render an entity query the way the store executes it: the paginated
    /// statement and, when requested, its separate count, each with its own
    /// placeholder sequence.
    fn build(query: &CompiledEntityQuery, names: &QueryNames) -> Built {
        let target = ContextTarget::Live;
        let entity_query = build_query(&target, query, names).expect("valid entity query");
        let (sql, values) = entity_query.select.build(PostgresQueryBuilder);
        let count_sql = entity_query.count.map(|count| count.build(PostgresQueryBuilder).0);
        Built {
            sql,
            count_sql,
            bind_values: values.0,
        }
    }

    fn ids(raw: &[&str]) -> Vec<Uri> {
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
        }
    }

    /// Attribute names alone, with no type resolved.
    fn attributes_only(attributes: ResolvedNames<AttributeName>) -> QueryNames {
        QueryNames {
            attributes,
            types: ResolvedNames::default(),
        }
    }

    /// A `GeoJSON` point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: &serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    /// A query with every filter absent, for tests that vary one part at a time.
    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build()
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build(&q, &known_types());
        assert!(built.sql.contains("SELECT"));
        assert!(built.sql.contains("\"payload\""));
        assert!(built.sql.contains("\"entities\""));
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
        assert!(built.count_sql.is_none());
    }

    #[test]
    fn build_empty_query_orders_by_id_for_stable_pagination() {
        // Clause 5.5.9: a paginated result needs a consistent order across
        // pages. A query with no `orderBy` must still emit the `id` tiebreaker
        // rather than leaving `LIMIT`/`OFFSET` over the plan's arbitrary order,
        // which can repeat and skip rows between pages.
        let q = empty_query();
        let built = build(&q, &known_types());
        assert!(built.sql.contains("ORDER BY \"entities\".\"id\""), "{}", built.sql);
    }

    /// A payload query that filters on nothing reads the identity row alone
    /// and fetches each page row's document by key, under the document column's
    /// name: the lookup is deferred past the sort and the limit, so a page of
    /// entities costs a page of document reads rather than a join over every
    /// candidate.
    #[test]
    fn a_payload_query_with_no_filter_joins_nothing_and_reads_the_page_documents_by_key() {
        let built = build(&empty_query(), &known_types());
        assert!(
            built.sql.starts_with(
                r#"SELECT (SELECT "payload" FROM "entity_documents" WHERE "entity_key" = "entities"."entity_key") AS "payload" FROM "entities" WHERE TRUE ORDER BY"#
            ),
            "{}",
            built.sql
        );
        assert!(!built.sql.contains("JOIN"), "{}", built.sql);
    }

    /// A `q` filter reads the payload for its jsonpath fallback, so the document
    /// row is joined and the projection reads the joined column.
    #[test]
    fn a_q_filter_joins_the_documents_and_projects_the_joined_column() {
        let mut q = empty_query();
        q.selection.q_expr = Some(parser::parse("\"https://example.com/temperature\">1").expect("valid q"));
        let built = build(&q, &temperature_is_known());
        assert!(built.sql.contains(r#"JOIN "entity_documents" ON"#), "{}", built.sql);
        assert!(
            built.sql.starts_with(r#"SELECT "entity_documents"."payload" AS "payload" FROM "entities""#),
            "{}",
            built.sql
        );
    }

    /// Clause 5.5.9 `attrs` is answered by the search row's interned attribute
    /// names, one containment test per term the GIN index serves; a name the
    /// dictionary never recorded is carried by no entity and matches nothing.
    #[test]
    fn attrs_terms_are_containment_tests_on_the_search_rows_attribute_keys() {
        let mut q = empty_query();
        q.selection.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/neverWritten")];
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(11))]),
            types: ResolvedNames::default(),
        };
        let built = build(&q, &names);
        assert!(built.sql.contains(r#"JOIN "entity_search" ON"#), "{}", built.sql);
        assert!(
            built.sql.contains(r#""entity_search"."attribute_name_keys" @> (ARRAY[11])"#),
            "the key is inlined so the planner can estimate it: {}",
            built.sql
        );
        assert!(built.sql.contains(" OR (FALSE)"), "the unresolved term is unsatisfiable: {}", built.sql);
        assert!(
            !built.sql.contains("payload\" ->") && !built.sql.contains(" ? "),
            "no payload key probe remains: {}",
            built.sql
        );
        assert!(
            !built.bind_values.iter().any(|bind| matches!(bind, Value::Int(Some(11)))),
            "the key is inlined, not bound: {:?}",
            built.bind_values
        );
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.selection.selectors = vec![EntitySelector::builder().ids(ids(&["urn:a", "urn:b"])).build()];
        let built = build(&q, &known_types());
        assert!(
            built.sql.contains("\"id\" = ANY("),
            "an id set binds as one array, not a placeholder per id: {}",
            built.sql
        );
        assert!(
            built.bind_values.iter().any(|b| matches!(b, Value::Array(_, _))),
            "the id set is one bound text[] value: {:?}",
            built.bind_values
        );
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
        assert!(built.sql.contains("FALSE"), "the statement is unsatisfiable: {}", built.sql);
    }

    /// The clause 5.5.14 narrowing is conjoined with the rest of the query, so
    /// the filters a map-scoped request still carries cannot widen it back.
    #[test]
    fn considering_no_entity_survives_beside_the_other_filters() {
        let mut q = empty_query();
        q.selection.considered = ConsideredEntities::NoEntity;
        q.selection.selectors = vec![EntitySelector::builder().ids(ids(&["urn:a"])).build()];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("FALSE"), "the statement stays unsatisfiable: {}", built.sql);
        assert!(built.sql.contains("\"id\" = ANY("), "the selector is still rendered: {}", built.sql);
    }

    /// A request that named no identifier at all considers whatever it selects,
    /// which is the case an empty identifier list must never be confused with.
    #[test]
    fn a_query_that_named_no_identifier_renders_no_unsatisfiable_term() {
        let built = build(&empty_query(), &known_types());
        assert!(!built.sql.contains("FALSE"), "an unconstrained query stays unconstrained: {}", built.sql);
    }

    /// A warm spatial prefilter feeds its candidate id set into the selector,
    /// and at scale that set can exceed PostgreSQL's 65535 parameter ceiling.
    /// The id membership must therefore bind as a single `text[]` (`= ANY($n)`)
    /// beside the single EWKB reference geometry, never one placeholder per id.
    #[test]
    fn a_near_query_with_a_candidate_id_set_binds_one_array_beside_the_geometry() {
        let mut q = empty_query();
        q.selection.selectors = vec![EntitySelector::builder().ids(ids(&["urn:a", "urn:b", "urn:c"])).build()];
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let built = build(&q, &known_types());
        assert!(built.sql.contains("\"id\" = ANY("), "id membership is a single array test: {}", built.sql);
        assert!(built.sql.contains("ST_GeomFromEWKB"), "the near predicate is present: {}", built.sql);
        assert_eq!(
            built.bind_values.iter().filter(|b| matches!(b, Value::Array(_, _))).count(),
            1,
            "the candidate id set is exactly one bound array parameter: {:?}",
            built.bind_values
        );
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse("urn:ngsi-ld:Vehicle:.*").unwrap()))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("\"id\" ~"));
    }

    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/Vehicle"))))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("\"entity_type_name_keys\" @> (ARRAY[1])"), "{}", built.sql);
        assert!(
            !built.bind_values.iter().any(|bind| matches!(bind, Value::Int(Some(1)))),
            "the resolved key is inlined, not bound: {:?}",
            built.bind_values
        );
    }

    /// A type the dictionary has never recorded was never carried by any
    /// entity, so a selector on it renders an unsatisfiable predicate.
    #[test]
    fn an_unknown_type_selector_matches_nothing() {
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/NeverWritten"))))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("WHERE FALSE"), "{}", built.sql);
        assert!(
            !built.sql.contains("entity_type_name_keys"),
            "no probe is rendered for an unresolved type: {}",
            built.sql
        );
    }

    #[test]
    fn build_with_mixed_type_expr() {
        let mut q = empty_query();
        // (Home AND Vehicle) OR Motorhome
        q.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Or(vec![
                    TypeExpr::And(vec![
                        TypeExpr::Type(iri("https://example.com/Home")),
                        TypeExpr::Type(iri("https://example.com/Vehicle")),
                    ]),
                    TypeExpr::Type(iri("https://example.com/Motorhome")),
                ])))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("\"entity_type_name_keys\" @> (ARRAY["));
        assert!(built.sql.contains("AND"));
        assert!(built.sql.contains("OR"));
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.pagination.count = CountPolicy::Emit;
        q.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/Vehicle"))))
                .build(),
        ];
        let built = build(&q, &known_types());
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().contains("COUNT(*)"));
    }

    #[test]
    fn a_union_of_selectors_ors_them_rather_than_cross_producting() {
        // Clause 5.2.33: a POST `entities[]` body is a union of independent
        // selectors, so `{id:A,type:T1}` and `{id:B,type:T2}` render
        // `(A AND T1) OR (B AND T2)` and never the cross product that would also
        // match `(A,T2)`.
        let mut q = empty_query();
        q.selection.selectors = vec![
            EntitySelector::builder()
                .ids(ids(&["urn:ngsi-ld:Entity:A"]))
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/T1"))))
                .build(),
            EntitySelector::builder()
                .ids(ids(&["urn:ngsi-ld:Entity:B"]))
                .type_expr(Some(TypeExpr::Type(iri("https://example.com/T2"))))
                .build(),
        ];
        let built = build(&q, &known_types());
        // Two parenthesised groups ORed together: `(A AND T1) OR (B AND T2)`,
        // each pairing one selector's id with its own type. The type keys are
        // bound as parameters, so the union shows in the structure, not the text.
        assert!(built.sql.contains(") OR ("), "the two selectors must be ORed as groups: {}", built.sql);
        assert_eq!(
            built.sql.matches("\"id\" = ANY(").count(),
            2,
            "each selector keeps its own single-array id filter: {}",
            built.sql
        );
        assert_eq!(
            built.sql.matches("\"entity_type_name_keys\" @> (ARRAY[").count(),
            2,
            "each selector keeps its own type filter: {}",
            built.sql
        );
        assert!(
            built.sql.contains("ARRAY[4]") && built.sql.contains("ARRAY[5]"),
            "both selector types survive as distinct inlined keys: {}",
            built.sql
        );
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
        q.selection.q_expr = Some(parser::parse("\"https://example.com/temperature\"").unwrap());
        let built = build(&q, &temperature_is_known());
        // A `q` existence predicate resolves through the current-state
        // projection: row presence is existence, since a tombstone never
        // reaches `current_attribute_values`.
        assert!(built.sql.contains("\"current_attribute_values\""), "{}", built.sql);
        assert!(!built.sql.contains("\"payload\"->0 ?"), "{}", built.sql);
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        cq.selection.q_expr = Some(Expr::Pred(Predicate::Ordering {
            path: QueryPath::Local(AttrPath::simple("https://example.com/temperature".parse().expect("valid attribute name"))),
            op: OrderingOp::Greater,
            rhs: Scalar::Number(50.into()),
        }));
        let built = build(&cq, &temperature_is_known());
        assert!(built.sql.contains("EXISTS"), "{}", built.sql);
        assert!(built.sql.contains("\"value_number\" >"), "{}", built.sql);
        assert!(
            !built.sql.contains("jsonb_path_query_first"),
            "value predicates no longer scan the payload: {}",
            built.sql
        );
    }

    #[test]
    fn count_sql_reuses_the_main_query_bind_vector() {
        // The repository binds one vector against both statements, so the
        // placeholder sequence has to be identical up to the pagination
        // parameters the count query omits.
        let mut cq = empty_query();
        cq.pagination.count = CountPolicy::Emit;
        cq.selection.q_expr = Some(Expr::Pred(Predicate::Ordering {
            path: QueryPath::Local(AttrPath::simple("https://example.com/temperature".parse().expect("valid attribute name"))),
            op: OrderingOp::Greater,
            rhs: Scalar::Number(50.into()),
        }));
        let built = build(&cq, &temperature_is_known());
        let count_sql = built.count_sql.expect("count sql");
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
        let main = placeholders(&built.sql);
        let counted = placeholders(&count_sql);
        assert_eq!(counted, main[..counted.len()], "count query must reuse the leading binds verbatim");
        assert_eq!(counted, vec!["1".to_string(), "2".to_string()], "attribute id, then value: {count_sql}");
    }

    #[test]
    fn build_with_limit_offset() {
        let mut q = empty_query();
        q.pagination.limit = 50;
        q.pagination.offset = 100;
        let built = build(&q, &known_types());
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
        // sea-query parameterizes limit/offset as bind values
        assert!(!built.bind_values.is_empty(), "should have bind values for limit/offset");
    }

    fn boundary(id: &str) -> Uri {
        id.parse().expect("valid URI")
    }

    /// Clause 5.5.9.1: a forward cursor pages past the boundary identifier over
    /// the default `id` order -- `WHERE id > $x ORDER BY id ASC LIMIT n`, and no
    /// `OFFSET`, because the keyset predicate replaces the skip.
    #[test]
    fn build_with_keyset_after_pages_forward_ascending_without_offset() {
        let mut q = empty_query();
        q.keyset = Some(PageCursor::After(boundary("urn:ngsi-ld:Building:001")));
        let built = build(&q, &known_types());
        assert!(built.sql.contains(r#""entities"."id" > $"#), "{}", built.sql);
        assert!(built.sql.contains(r#"ORDER BY "entities"."id" ASC"#), "{}", built.sql);
        assert!(built.sql.contains("LIMIT"), "{}", built.sql);
        assert!(!built.sql.contains("OFFSET"), "a keyset page never skips: {}", built.sql);
        assert!(
            built
                .bind_values
                .iter()
                .any(|value| matches!(value, Value::String(Some(text)) if text.as_str() == "urn:ngsi-ld:Building:001")),
            "the boundary id is bound, not interpolated: {:?}",
            built.bind_values
        );
    }

    /// Clause 5.5.9.1: a backward cursor pages before the boundary over the
    /// reversed order -- `WHERE id < $x ORDER BY id DESC LIMIT n` -- which the
    /// store reverses to ascending after fetching.
    #[test]
    fn build_with_keyset_before_pages_backward_descending_without_offset() {
        let mut q = empty_query();
        q.keyset = Some(PageCursor::Before(boundary("urn:ngsi-ld:Building:042")));
        let built = build(&q, &known_types());
        assert!(built.sql.contains(r#""entities"."id" < $"#), "{}", built.sql);
        assert!(built.sql.contains(r#"ORDER BY "entities"."id" DESC"#), "{}", built.sql);
        assert!(!built.sql.contains("OFFSET"), "{}", built.sql);
    }

    /// Clause 5.5.9.1: the from-end cursor takes the final page -- `ORDER BY id
    /// DESC LIMIT n`, no boundary predicate -- which the store reverses to
    /// ascending. This is the Andromeda `last` anchor.
    #[test]
    fn build_with_keyset_end_takes_the_final_page_descending() {
        let mut q = empty_query();
        q.keyset = Some(PageCursor::End);
        let built = build(&q, &known_types());
        assert!(built.sql.contains(r#"ORDER BY "entities"."id" DESC"#), "{}", built.sql);
        assert!(
            !built.sql.contains(r#""entities"."id" >"#) && !built.sql.contains(r#""entities"."id" <"#),
            "no boundary predicate: {}",
            built.sql
        );
        assert!(built.sql.contains("LIMIT"), "{}", built.sql);
        assert!(!built.sql.contains("OFFSET"), "{}", built.sql);
    }

    /// The ids-only statement pages by keyset the same way the payload one does.
    #[test]
    fn build_ids_query_pages_by_keyset() {
        let mut q = empty_query();
        q.keyset = Some(PageCursor::After(boundary("urn:ngsi-ld:Building:001")));
        let target = ContextTarget::Live;
        let (sql, _values) = build_ids_query(&target, &q, &known_types())
            .expect("valid ids query")
            .build(PostgresQueryBuilder);
        assert!(sql.contains(r#""entities"."id" > $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "entities"."id" ASC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// Clause 4.23.1 sorts by the attribute's value, which the statement reads
    /// out of the payload, and clause 4.23.2 ranks that value's datatype before
    /// comparing it.
    #[test]
    fn build_with_order_by_asc() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple("https://example.com/name".parse().expect("valid attribute name"))),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("ORDER BY"));
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("jsonb_path_query_first("), "{}", built.sql);
        assert!(built.sql.contains("https://example.com/name"), "{}", built.sql);
        assert!(built.sql.contains("array_position("), "the datatype rank decides first: {}", built.sql);
    }

    #[test]
    fn build_with_order_by_collation() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple("https://example.com/name".parse().expect("valid attribute name"))),
            direction: OrderDirection::Asc,
            collation: Some(IcuCollation::parse("und-u-ks-identic").expect("valid collation")),
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("COLLATE \"und-u-ks-identic\""));
        assert!(built.sql.contains("ASC"));
    }

    /// Clause 4.8 with clause 4.23: `orderBy=createdAt` sorts on the identity
    /// row's own `created_at` column, not on a payload attribute, so the
    /// statement reads `entities.created_at` and joins no document row for it.
    #[test]
    fn build_with_order_by_created_at_reads_the_identity_column() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::CreatedAt,
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains(r#"ORDER BY "entities"."created_at" ASC"#), "{}", built.sql);
        assert!(!built.sql.contains("jsonb_path_query_first("), "no payload lateral is opened: {}", built.sql);
    }

    /// A descending `createdAt` reverses only the order.
    #[test]
    fn build_with_order_by_created_at_desc_reverses_the_order() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::CreatedAt,
            direction: OrderDirection::Desc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains(r#"ORDER BY "entities"."created_at" DESC"#), "{}", built.sql);
    }

    /// Clause 4.8 `modifiedAt` lives on `entity_documents`, is nullable, and so
    /// sorts with `NULLS LAST`; the term joins the document row to read it.
    #[test]
    fn build_with_order_by_modified_at_joins_and_reads_the_document_column() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::ModifiedAt,
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains(r#"JOIN "entity_documents" ON"#), "the document row is joined: {}", built.sql);
        assert!(
            built.sql.contains(r#"ORDER BY "entity_documents"."modified_at" ASC NULLS LAST"#),
            "{}",
            built.sql
        );
    }

    #[test]
    fn build_with_order_by_dist_asc_with_geo() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("ST_DistanceSphere("), "{}", built.sql);
        assert!(built.sql.contains("ASC"));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_returns_error() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let target = ContextTarget::Live;
        let result = build_query(&target, &q, &known_types());
        assert!(result.is_err());
    }

    #[test]
    fn build_with_order_by_dist_with_ordering_geometry() {
        let mut q = empty_query();
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("ST_GeomFromEWKB"));
        assert!(
            built.bind_values.iter().any(|b| matches!(b, Value::Bytes(_))),
            "reference geometry must be bound as EWKB bytes, not a GeoJSON string"
        );
    }

    #[test]
    fn build_with_order_by_dist_ordering_geometry_takes_precedence() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([2.3522, 48.8566])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("ST_Distance"));
        // Ordering geometry (Berlin) takes precedence over the geo query (Paris);
        // since reference geometries are now bound as EWKB bytes, verify that the
        // bind values carry exactly one raw geometry blob for the distance expression.
        assert!(built.sql.contains("ST_GeomFromEWKB"));
        assert!(built.bind_values.iter().any(|b| matches!(b, Value::Bytes(_))));
    }

    /// Clause 4.23.1 orders on distance over the sphere (Haversine), and the
    /// spherical measurement ignores any third ordinate. An `orderGeometry`
    /// that carries an altitude must therefore render exactly the statement a
    /// flat one renders; only the bound reference differs, because the bind
    /// keeps the altitude it was given.
    #[test]
    fn a_three_dimensional_ordering_geometry_renders_the_same_distance_sql() {
        let ordering_term = CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            direction: OrderDirection::DistAsc,
            collation: None,
        };

        let mut flat_query = empty_query();
        flat_query.ordering.geometry = Some(OrderingGeometry::new(point(&serde_json::json!([13.3986, 52.5547]))));
        flat_query.ordering.terms = vec![ordering_term.clone()]; // clone: the elevated query orders the same way
        let flat = build(&flat_query, &known_types());

        let mut elevated_query = empty_query();
        elevated_query.ordering.geometry = Some(OrderingGeometry::new(point(&serde_json::json!([13.3986, 52.5547, 34.5]))));
        elevated_query.ordering.terms = vec![ordering_term];
        let elevated = build(&elevated_query, &known_types());

        assert_eq!(elevated.sql, flat.sql, "distance ordering must not vary with the reference dimensionality");
        assert!(elevated.sql.contains("ST_DistanceSphere("), "{}", elevated.sql);
        assert!(
            !elevated.sql.contains("::geography"),
            "the spheroidal measurement must not be used: {}",
            elevated.sql
        );

        let bytes = |built: &Built| -> Vec<Vec<u8>> {
            built
                .bind_values
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
                    | Value::Json(_)
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
        assert_ne!(bytes(&elevated), bytes(&flat), "the reference must still bind the altitude it was given");
    }

    #[test]
    fn geo_default_location_includes_expiration_guard() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("5000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([13.4, 52.5])),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let built = build(&q, &known_types());
        assert!(built.sql.contains("ST_DWithin"), "should contain spatial predicate");
        assert!(
            built
                .sql
                .contains(r#""entity_search"."location_expires_at" IS NULL OR "entity_search"."location_expires_at" > NOW()"#),
            "the guard reads the search row's expiry column: {}",
            built.sql
        );
        assert!(!built.sql.contains("expiresAt"), "the guard no longer walks the payload: {}", built.sql);
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

        for georel in variants {
            let mut q = empty_query();
            q.selection.geo = Some(GeoQuery {
                georel: georel.clone(), // clone: the relation names the failing case in every assertion below
                geometry: point(&serde_json::json!([1.0, 2.0])),
                geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
            });
            let built = build(&q, &known_types());
            assert!(built.sql.contains("ST_GeomFromEWKB"), "{georel:?} SQL missing ST_GeomFromEWKB");
            // Default-location path must not re-decode GeoJSON for the reference side.
            assert!(
                !built.sql.contains("ST_GeomFromGeoJSON"),
                "{georel:?} SQL still contains ST_GeomFromGeoJSON: {}",
                built.sql
            );
            assert!(
                built.bind_values.iter().any(|b| matches!(b, Value::Bytes(_))),
                "{georel:?} missing EWKB bytes bind"
            );
        }
    }

    /// Clause 4.10: `geoproperty` selects any `GeoProperty` of the entity. A
    /// named one is held in `current_attribute_values`, so the whole statement —
    /// not just the fragment — has to reach it there.
    #[test]
    fn a_named_geoproperty_filters_through_its_attribute_instance_rows() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/customGeo".parse().expect("valid attribute name"),
        });
        let names = attributes_only(ResolvedNames::from_pairs([(
            "https://example.com/customGeo".parse().expect("valid attribute name"),
            AttributeNameKey::from(42_i32),
        )]));
        let built = build(&q, &names);
        assert!(built.sql.contains("EXISTS(SELECT 1 FROM \"current_attribute_values\""), "{}", built.sql);
        assert!(built.sql.contains("\"attribute_name_key\" = "), "{}", built.sql);
        assert!(
            built.bind_values.iter().any(|bind| matches!(bind, Value::Int(Some(42)))),
            "the dictionary key selects the attribute: {:?}",
            built.bind_values
        );
        assert!(built.sql.contains("ST_GeomFromEWKB"), "reference side must still bind EWKB");
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching, so an unrecorded `GeoProperty` yields an empty result rather
    /// than an error.
    #[test]
    fn an_unrecorded_geoproperty_filters_everything_out_without_failing() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/neverRecorded".parse().expect("valid attribute name"),
        });
        let built = build(&q, &known_types());
        assert!(built.sql.contains("FALSE"), "{}", built.sql);
    }

    #[test]
    fn scope_complex_generates_mixed() {
        let mut q = empty_query();
        q.selection.scope_q = Some(ScopeQuery::parse("/A;/B,/C/#").expect("valid scope query"));
        let built = build(&q, &known_types());
        assert!(built.sql.contains(" AND "), "SQL should contain AND");
        assert!(built.sql.contains(" OR "), "SQL should contain OR");
        assert!(built.sql.contains("::lquery"), "SQL should cast to lquery");
    }

    // -- SQL literal escaping ------------------------------------------------

    /// Number of `'` characters in a statement. Every literal contributes an
    /// opening and a closing quote, and a doubled apostrophe contributes two, so
    /// a balanced statement always has an even count. An unescaped apostrophe
    /// inside a literal makes it odd.
    fn apostrophe_count(sql: &str) -> usize {
        sql.matches('\'').count()
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
        for georel in relations {
            let mut q = empty_query();
            q.selection.geo = Some(GeoQuery {
                georel: georel.clone(), // clone: the relation is named again in the assertion
                geometry: point(&serde_json::json!([1.0, 2.0])),
                geoproperty: "https://example.com/o'brien".parse().expect("valid attribute name"),
            });
            let built = build(&q, &names);
            assert!(!built.sql.contains("brien"), "{georel:?}: {}", built.sql);
            assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
        }
    }

    #[test]
    fn an_order_by_attribute_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple("https://example.com/o'brien".parse().expect("valid attribute name"))),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build(&q, &known_types());
        assert!(built.sql.contains("o''brien"), "{}", built.sql);
        assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
    }

    /// A distance ordering measures through the dictionary key, so no IRI
    /// reaches its distance subquery. Clause 4.23.2 still ranks the entities
    /// whose attribute is not a `GeoProperty` by value, and that comparison
    /// interpolates the IRI into a jsonpath, where an apostrophe in it is
    /// doubled rather than closing the literal.
    #[test]
    fn a_distance_ordering_escapes_the_geoproperty_iri_its_value_comparison_reads() {
        let mut q = empty_query();
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(AttrPath::simple("https://example.com/o'brien".parse().expect("valid attribute name"))),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let names = attributes_only(ResolvedNames::from_pairs([(
            "https://example.com/o'brien".parse().expect("valid attribute name"),
            AttributeNameKey::from(9_i32),
        )]));
        let built = build(&q, &names);
        assert!(built.sql.contains("MIN(ST_DistanceSphere("), "the nearest instance decides: {}", built.sql);
        assert!(
            built.sql.contains("\"attribute_name_key\" = $"),
            "the geometry is reached by dictionary key: {}",
            built.sql
        );
        assert!(built.sql.contains("o''brien"), "{}", built.sql);
        assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
    }
}
