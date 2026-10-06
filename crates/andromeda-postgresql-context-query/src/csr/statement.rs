//! SQL generation for Context Source Registration discovery (clause 5.10.2).

use crate::csr::{
    attribute_specification::attribute_names_declared,
    dataset_rule::dataset_rule,
    encountered_sources::not_encountered,
    entity_specification::EntitySpecification,
    expiry_predicate::not_expired,
    filter::csr_expr_to_condition,
    projection::{attribute_name_candidate, entity_id_candidate, entity_type_candidate},
    registration_select::registration_select,
    temporal_partition::temporal_partition,
    unnesting::{entity_axis_covers, information_item_matches},
};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, geo::query::GeoQuery, page_window::PageWindow, read_order::ReadOrder};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBSERVATION_SPACE, OPERATION_SPACE};
use andromeda_postgresql_query::{
    dictionary::resolved_names::ResolvedNames,
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::GeometrySource,
    page_window::apply_identifier_window,
    scope_predicate::{ScopeTarget, condition as scope_condition},
};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::aggregate::count_all;
use sea_query::{Condition, Expr as SeaExpr, Query, SelectStatement};

/// A compiled CSR discovery query: the paginated result statement and its count,
/// each a statement tree the store binds and executes on its own.
///
/// A CSR discovery always reports a total, and a paginated result and its
/// unpaginated count cannot share one placeholder sequence, so the two are
/// separate statements.
pub struct CsrQuery {
    /// The paginated statement selecting the matching registrations.
    pub select: SelectStatement,
    /// The `COUNT` over the same filter.
    pub count: SelectStatement,
    /// The order the page's rows come back in, which the store presents
    /// ascending.
    pub read_order: ReadOrder,
}

/// Builds a parameterized SQL query to discover the page `window` selects of
/// the matching Context Source Registrations, in identifier order.
///
/// The entity specification and the attribute list are matched against the
/// registrations' `information` as clause 5.12 describes; the context source
/// filter, geoquery, scope query, dataset identifiers, encountered sources and
/// temporal window are matched against the registrations' own members (clause
/// 5.10.2.4).
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the geoquery's
/// reference geometry has no EWKB spelling the store can bind, and
/// [`RenderError::Query`] when `georel=near` names neither `maxDistance` nor
/// `minDistance`, which clause 4.10 requires at least one of. A discovery
/// carries no ordering terms, so no other fragment of the statement can fail
/// to render.
pub fn build_csr_query(query: &CompiledCsrQuery, window: &PageWindow) -> Result<CsrQuery, RenderError> {
    let condition = build_csr_conditions(query)?;

    let mut select = registration_select();
    select
        // clone: the count statement filters on the same condition
        .cond_where(condition.clone());
    // Clause 5.5.9: the registrations are paged in identifier order in either
    // pagination mode. The identifier is unique, so an offset page is
    // deterministic across executions, and a cursor continues from the very
    // order its first page was read in.
    let read_order = apply_identifier_window(&mut select, SeaExpr::col(Csr::Id), window);

    // Count query always generated for CSR
    let mut count = Query::select();
    count.expr(count_all()).from(Csr::Table).cond_where(condition);

    Ok(CsrQuery {
        select: select.take(),
        count: count.take(),
        read_order,
    })
}

/// Builds sea-query Condition from compiled CSR query filters.
fn build_csr_conditions(query: &CompiledCsrQuery) -> Result<Condition, RenderError> {
    // Clause 5.9.2.4: an expired registration is deleted the moment its
    // `expiresAt` is reached, so discovery never reports one.
    let mut condition = Condition::all().add(not_expired());

    // Clause 5.12: the entity specification and the attribute names must match
    // the same `RegistrationInfo`. The projection preselects narrow the
    // candidates per dimension; the `information` unnesting decides.
    let entities = EntitySpecification {
        entity_type: query.entity_type.as_ref(),
        ids: &query.ids,
        id_pattern: query.id_pattern.as_ref(),
    };
    if let Some(entity_type) = &query.entity_type {
        condition = condition.add(entity_type_candidate(entity_type));
    }
    if !query.ids.is_empty() {
        condition = condition.add(entity_id_candidate(&query.ids));
    }
    if !query.attrs.is_empty() {
        condition = condition.add(attribute_name_candidate(&query.attrs));
    }
    if entities.constrains() || !query.attrs.is_empty() {
        let mut item = Condition::all();
        if entities.constrains() {
            item = item.add(entity_axis_covers(entities.matching_entities()));
        }
        if !query.attrs.is_empty() {
            item = item.add(attribute_names_declared(&query.attrs));
        }
        condition = condition.add(information_item_matches(item));
    }

    // Clause 4.9: the context source filter reads the registration's own
    // Context Source Properties.
    if let Some(expr) = &query.csf_expr {
        condition = condition.add(csr_expr_to_condition(expr));
    }

    // Geo filter: a registration that declares no location still matches
    // (clause 5.10.2.4).
    if let Some(geo) = &query.geo {
        condition = condition.add(geo_sql::condition(
            geo,
            GeometrySource::Registration {
                column: registration_geometry_column(geo),
            },
            &ResolvedNames::default(),
        )?);
    }

    // Clause 4.19: a registration's scopes are stored as the same `ltree`
    // label paths current entities carry, so a `scopeQ` compiles to the same
    // `lquery` match.
    if let Some(scope_q) = &query.scope_q {
        condition = condition.add(scope_condition(scope_q, &ScopeTarget::current_paths((Csr::Table, Csr::ScopePaths))));
    }

    if !query.dataset_id.is_empty() {
        condition = condition.add(dataset_rule(&query.dataset_id));
    }

    if !query.encountered_sources.is_empty() {
        condition = condition.add(not_encountered(&query.encountered_sources));
    }

    condition = condition.add(temporal_partition(query.temporal.as_ref()));

    Ok(condition)
}

/// The registration geometry a geoquery is matched against.
///
/// A registration carries three geometries (Table 5.2.9-1). The query's
/// `geoproperty` selects `observationSpace` or `operationSpace` when it names
/// one of them; every other `GeoProperty`, including the clause 5.10.2.4 default
/// `location`, is matched against `location`.
fn registration_geometry_column(geo: &GeoQuery) -> &'static str {
    let geoproperty = geo.geoproperty.as_str();
    if geoproperty == OBSERVATION_SPACE.as_str() {
        "observation_space"
    } else if geoproperty == OPERATION_SPACE.as_str() {
        "operation_space"
    } else {
        "location"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        csr::compiled::CompiledCsrTemporalFilter,
        cursor::PageCursor,
        dataset::DatasetSelector,
        filter::parser,
        geo::query::GeoQuery,
        page_window::{PageStart, PageWindow},
        pattern::Pattern,
        scope::query::ScopeQuery,
        temporal::window::TimeInterval,
    };
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// A built CSR discovery rendered with its values inlined: the paginated
    /// statement and its always-present count.
    struct Built {
        sql: String,
        count_sql: String,
    }

    fn build(query: &CompiledCsrQuery) -> Built {
        build_paged(query, &PageWindow::offset(20, 0))
    }

    fn build_paged(query: &CompiledCsrQuery, window: &PageWindow) -> Built {
        let csr = build_csr_query(query, window).expect("a renderable discovery");
        Built {
            sql: csr.select.to_string(PostgresQueryBuilder),
            count_sql: csr.count.to_string(PostgresQueryBuilder),
        }
    }

    fn ids(raw: &[&str]) -> Vec<Identifier> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn dt(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// A `GeoJSON` point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: &serde_json::Value) -> Geometry {
        serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    fn geo_query(geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: geoproperty.parse().expect("valid attribute name"),
        }
    }

    /// A discovery query with every filter absent, for tests that vary one at
    /// a time.
    fn empty_query() -> CompiledCsrQuery {
        CompiledCsrQuery::builder().build()
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build(&q);
        assert!(built.sql.contains("context_source_registrations"));
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
    }

    /// Clause 5.9.2.4: a registration past its `expiresAt` is gone from
    /// discovery whether or not the sweeper has deleted its row yet.
    #[test]
    fn every_discovery_excludes_expired_registrations_from_the_page_and_the_count() {
        let built = build(&empty_query());
        assert!(built.sql.contains("\"expires_at\" IS NULL OR \"expires_at\" > NOW()"), "{}", built.sql);
        assert!(
            built.count_sql.contains("\"expires_at\" IS NULL OR \"expires_at\" > NOW()"),
            "{}",
            built.count_sql
        );
    }

    #[test]
    fn discovery_reads_the_shared_registration_projection() {
        let built = build(&empty_query());
        for column in ["\"id\"", "\"registration_name\"", "\"jsonld_context\"", "\"additional_properties\""] {
            assert!(built.sql.contains(column), "missing {column}: {}", built.sql);
        }
        assert!(
            built
                .sql
                .contains("ST_AsGeoJSON(\"context_source_registrations\".\"location\")::jsonb AS \"location\""),
            "{}",
            built.sql
        );
        assert!(built.sql.contains("LEFT JOIN \"registration_delivery_state\""), "{}", built.sql);
        assert!(!built.sql.contains("\"type\""), "{}", built.sql);
    }

    /// Clause 5.5.9: an offset page reads the registrations in the unique
    /// identifier order, so it is deterministic across executions.
    #[test]
    fn an_offset_discovery_orders_by_id() {
        let built = build_paged(&empty_query(), &PageWindow::offset(10, 30));
        assert!(built.sql.contains(r#"ORDER BY "id" ASC LIMIT 10 OFFSET 30"#), "{}", built.sql);
        assert!(!built.sql.contains("created_at\" ASC"), "{}", built.sql);
    }

    /// The first page of a cursor walk reads in the identifier order its
    /// cursors continue from, never by `created_at`.
    #[test]
    fn a_fresh_discovery_orders_by_id() {
        let window = PageWindow {
            limit: 3,
            start: PageStart::Cursor(None),
        };
        let built = build_paged(&empty_query(), &window);
        assert!(built.sql.contains(r#"ORDER BY "id" ASC LIMIT 3"#), "{}", built.sql);
        assert!(!built.sql.contains(r#"ORDER BY "created_at""#), "{}", built.sql);
        assert!(!built.sql.contains("OFFSET"), "{}", built.sql);
    }

    /// Clause 5.5.9.1: a forward cursor pages `id > boundary` ascending over the
    /// registration id, replacing the offset window.
    #[test]
    fn a_forward_cursor_pages_id_ascending_past_the_boundary() {
        let built = build_paged(&empty_query(), &PageWindow::after(20, "urn:ngsi-ld:Csr:010".parse().expect("valid URI")));
        assert!(built.sql.contains(r#""id" > 'urn:ngsi-ld:Csr:010'"#), "{}", built.sql);
        assert!(built.sql.contains(r#"ORDER BY "id" ASC"#), "{}", built.sql);
        assert!(!built.sql.contains("OFFSET"), "keyset mode drops OFFSET: {}", built.sql);
        assert!(!built.sql.contains(r#"ORDER BY "created_at""#), "keyset mode orders by id alone: {}", built.sql);
    }

    /// Clause 5.5.9.1: a backward cursor pages `id < boundary` descending (the
    /// store reverses the rows to ascending for display).
    #[test]
    fn a_backward_cursor_pages_id_descending_before_the_boundary() {
        let window = PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(PageCursor::Before("urn:ngsi-ld:Csr:010".parse().expect("valid URI")))),
        };
        let built = build_paged(&empty_query(), &window);
        assert!(built.sql.contains(r#""id" < 'urn:ngsi-ld:Csr:010'"#), "{}", built.sql);
        assert!(built.sql.contains(r#"ORDER BY "id" DESC"#), "{}", built.sql);
        assert!(!built.sql.contains("OFFSET"), "{}", built.sql);
    }

    /// Clause 5.5.9.1: the from-end cursor pages the last ids descending with no
    /// boundary predicate.
    #[test]
    fn a_from_end_cursor_pages_the_last_ids_descending() {
        let window = PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(PageCursor::End)),
        };
        let built = build_paged(&empty_query(), &window);
        assert!(built.sql.contains(r#"ORDER BY "id" DESC"#), "{}", built.sql);
        assert!(
            !built.sql.contains(r#""id" >"#) && !built.sql.contains(r#""id" <"#),
            "no boundary predicate: {}",
            built.sql
        );
        assert!(!built.sql.contains("OFFSET"), "{}", built.sql);
    }

    #[test]
    fn build_with_entity_type() {
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        let built = build(&q);
        assert!(
            built.sql.contains("jsonb_exists(\"ent\".\"value\" -> ('type'), 'https://example.com/Vehicle')"),
            "{}",
            built.sql
        );
    }

    /// Clause 5.12: entity identifiers are matched against the registered
    /// `EntityInfo` entries, never against the registration's own identifier.
    #[test]
    fn entity_ids_are_matched_against_entity_info_not_the_registration_id() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a", "urn:b"]);
        let built = build(&q);
        assert!(built.sql.contains("\"ri\".\"entity_id\" IN ('urn:a', 'urn:b', '')"), "{}", built.sql);
        assert!(
            built.sql.contains("(\"ent\".\"value\" ->> ('id')) = ANY(ARRAY ['urn:a','urn:b'])"),
            "{}",
            built.sql
        );
        assert!(!built.sql.contains("\"id\" = ANY("), "the registration id is not an entity id: {}", built.sql);
    }

    #[test]
    fn a_queried_id_pattern_is_evaluated_against_registered_ids_and_admits_registered_patterns() {
        let mut q = empty_query();
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        let built = build(&q);
        assert!(built.sql.contains("(\"ent\".\"value\" ->> ('id')) ~ 'urn:ngsi-ld:.*'"), "{}", built.sql);
        assert!(built.sql.contains("'idPattern'"), "{}", built.sql);
        assert!(!built.sql.contains("\"ri\".\"entity_id_pattern\" = "), "{}", built.sql);
    }

    /// Clause 5.12: an `EntityInfo` with neither `id` nor `idPattern` matches
    /// every identifier; the first disjunct of the identifier axis admits it.
    #[test]
    fn an_entity_info_without_identifier_constraint_is_admitted_for_any_queried_id() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a"]);
        let built = build(&q);
        assert!(
            built
                .sql
                .contains("(NOT jsonb_exists(\"ent\".\"value\", 'id')) AND (NOT jsonb_exists(\"ent\".\"value\", 'idPattern'))"),
            "{}",
            built.sql
        );
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/temp")];
        let built = build(&q);
        assert!(built.sql.contains("propertyNames"));
        assert!(built.sql.contains("relationshipNames"));
    }

    /// Clause 5.12: an information item naming no property and no relationship
    /// registered entities only, and matches any attribute list.
    #[test]
    fn an_entity_only_information_item_matches_any_attribute_list() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert!(
            built
                .sql
                .contains("(NOT jsonb_exists(\"info\".\"value\", 'propertyNames')) AND (NOT jsonb_exists(\"info\".\"value\", 'relationshipNames'))"),
            "{}",
            built.sql
        );
    }

    /// Clause 5.12: the entity axis and the attribute axis must hold for the
    /// same `RegistrationInfo`, so both are conjoined inside one unnesting.
    #[test]
    fn entity_and_attribute_axes_are_decided_on_the_same_information_item() {
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert_eq!(built.sql.matches("jsonb_array_elements(\"information\")").count(), 1, "{}", built.sql);
    }

    #[test]
    fn the_selective_filters_preselect_against_the_routing_projection() {
        // Type, identifiers and attribute names each gain an indexed semi-join
        // over the projection; the `information` unnesting they are conjoined
        // with still decides membership, so the preselect only has to be a
        // superset of it.
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert!(
            built.sql.contains("\"ri\".\"entity_types\" && ARRAY ['https://example.com/Vehicle','*']"),
            "{}",
            built.sql
        );
        assert!(built.sql.contains("\"ri\".\"entity_id\" IN ('urn:ngsi-ld:Vehicle:1', '')"), "{}", built.sql);
        assert!(built.sql.contains("\"ri\".\"property_name_keys\" && array_append("), "{}", built.sql);
        assert!(!built.sql.contains("NOT EXISTS"), "{}", built.sql);
        assert!(built.sql.contains("jsonb_array_elements(\"information\")"), "{}", built.sql);
    }

    #[test]
    fn a_query_without_selective_filters_reads_no_projection() {
        // Nothing to narrow: an unfiltered discovery must not pay for a
        // semi-join that would admit every registration anyway.
        let built = build(&empty_query());
        assert!(!built.sql.contains("registration_information"), "{}", built.sql);
    }

    #[test]
    fn the_count_statement_carries_the_same_preselect_as_the_page() {
        // Clause 5.5.9: the total must count exactly the rows the page draws
        // from, so both statements share one condition.
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert!(built.count_sql.contains("registration_information"), "{}", built.count_sql);
    }

    #[test]
    fn build_with_geo_within() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        let built = build(&q);
        // Clause 5.10.2: a registration with no location still matches.
        assert!(built.sql.contains("\"location\" IS NULL"), "{}", built.sql);
        assert!(built.sql.contains("ST_Within"), "{}", built.sql);
    }

    /// Table 5.2.9-1: `observationSpace` and `operationSpace` are geometries of
    /// their own, reached when the geoquery names them; any other `GeoProperty`,
    /// including the clause 5.10.2.4 default, reads `location`.
    #[test]
    fn the_geoproperty_selects_the_registration_geometry() {
        for (geoproperty, column) in [
            ("https://uri.etsi.org/ngsi-ld/location", "\"location\""),
            ("https://uri.etsi.org/ngsi-ld/observationSpace", "\"observation_space\""),
            ("https://uri.etsi.org/ngsi-ld/operationSpace", "\"operation_space\""),
            ("https://example.com/somewhere", "\"location\""),
        ] {
            let mut q = empty_query();
            q.geo = Some(geo_query(geoproperty));
            let built = build(&q);
            assert!(built.sql.contains(&format!("{column} IS NULL")), "{geoproperty}: {}", built.sql);
        }
    }

    /// Clause 5.10.2.4: without a temporal query only registrations without
    /// any interval are considered.
    #[test]
    fn no_temporal_query_admits_only_latest_information_registrations() {
        let built = build(&empty_query());
        assert!(
            built.sql.contains("\"observation_interval\" IS NULL AND \"management_interval\" IS NULL"),
            "{}",
            built.sql
        );
    }

    #[test]
    fn build_with_temporal_before() {
        let mut q = empty_query();
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .interval(TimeInterval::Before(dt("2024-01-01T00:00:00Z")))
                .timeproperty(None)
                .build(),
        );
        let built = build(&q);
        assert!(built.sql.contains("\"observation_interval\" IS NOT NULL"), "{}", built.sql);
        assert!(built.sql.contains("startAt"));
        assert!(!built.sql.contains("management_interval\" IS NULL"), "{}", built.sql);
    }

    #[test]
    fn build_with_temporal_after() {
        let mut q = empty_query();
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .interval(TimeInterval::After(dt("2024-01-01T00:00:00Z")))
                .timeproperty(Some(TimeProp::Created))
                .build(),
        );
        let built = build(&q);
        assert!(built.sql.contains("\"management_interval\" IS NOT NULL"), "{}", built.sql);
        assert!(built.sql.contains("endAt"));
    }

    #[test]
    fn build_with_temporal_between() {
        let mut q = empty_query();
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .interval(TimeInterval::Between {
                    from: dt("2024-01-01T00:00:00Z"),
                    to: dt("2024-02-01T00:00:00Z"),
                })
                .timeproperty(None)
                .build(),
        );
        let built = build(&q);
        assert!(built.sql.contains("startAt"));
        assert!(built.sql.contains("endAt"));
    }

    /// Clause 4.9: `csf` reads the registration's Context Source Properties.
    #[test]
    fn build_with_csf_existence() {
        let mut q = empty_query();
        q.csf_expr = Some(parser::parse("temperature").unwrap());
        let built = build(&q);
        assert!(built.sql.contains("\"additional_properties\" ?"));
    }

    #[test]
    fn a_scope_query_matches_the_stored_label_paths_with_an_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::parse("/Madrid/#").unwrap());
        let built = build(&q);
        assert!(built.sql.contains("\"context_source_registrations\".\"scope_paths\" ~"), "{}", built.sql);
        assert!(built.sql.contains("'Madrid.*'::lquery"), "{}", built.sql);
        assert!(!built.sql.contains("UNNEST"), "{}", built.sql);
    }

    /// Clause 5.12: a registration declaring no `datasetId` matches; one that
    /// does must share a value with the request.
    #[test]
    fn build_with_dataset_ids() {
        let mut q = empty_query();
        q.dataset_id = vec![DatasetSelector::Default];
        let built = build(&q);
        assert!(
            built.sql.contains("\"dataset_id\" IS NULL OR (\"dataset_id\" && ARRAY ['@none'])"),
            "{}",
            built.sql
        );
    }

    /// Clause 5.12: a registration whose alias the request already passed
    /// through never matches.
    #[test]
    fn build_with_encountered_sources() {
        let mut q = empty_query();
        q.encountered_sources = vec![ContextSourcePseudonym::new("broker-b").unwrap()];
        let built = build(&q);
        assert!(built.sql.contains("(NOT \"context_source_alias\" = ANY(ARRAY ['broker-b']))"), "{}", built.sql);
    }

    #[test]
    fn build_count_sql_always_present() {
        let q = empty_query();
        let built = build(&q);
        assert!(built.count_sql.contains("COUNT(*)"), "{}", built.count_sql);
    }

    #[test]
    fn build_with_pagination() {
        let built = build_paged(&empty_query(), &PageWindow::offset(50, 100));
        assert!(built.sql.contains("LIMIT 50 OFFSET 100"), "{}", built.sql);
    }

    #[test]
    fn build_with_multiple_filters() {
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        q.ids = ids(&["urn:a"]);
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert!(built.sql.contains(" AND "));
    }

    /// Registrations read clause 4.10 exactly as entities do: the reference
    /// geometry is parsed once during compilation, so a polygon reaches this
    /// builder already validated against IETF RFC 7946 and there is nothing
    /// left for the CSR path to repair.
    #[test]
    fn a_polygon_reference_geometry_renders_the_same_relation_as_a_point() {
        let mut q = empty_query();
        let ring = serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]]
        });
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(ring).expect("polygon parses"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let built = build(&q);
        assert!(built.sql.contains("ST_Within"), "{}", built.sql);
        assert!(built.sql.contains("ST_GeomFromEWKB"), "{}", built.sql);
    }
}
