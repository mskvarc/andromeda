//! SQL generation for Context Source Registration discovery (clause 5.10.2).

use crate::csr::{
    encountered_sources::not_encountered,
    entity_specification::row_matches,
    expiry_predicate::not_expired,
    filter::csr_expr_to_condition,
    geometry_parts::{GeometryParts, geometry_parts},
    information_row::some_information_row,
    preselect::attribute_name_candidate,
    registration_select::registration_select,
    temporal_partition::temporal_partition,
};
use andromeda_ngsi_ld_query::{
    csr::{compiled::CompiledCsrQuery, expanded_entity_specification::ExpandedEntitySpecification},
    page_window::PageWindow,
    read_order::ReadOrder,
};
use andromeda_postgresql_query::{
    error::RenderError,
    page_window::apply_identifier_window,
    scope_predicate::{ScopeTarget, condition as scope_condition},
};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    pg_array::{any, array_cat, array_of_subquery},
    scalar_subquery::scalar_subquery,
};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

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
/// reference geometry has no EWKB spelling the store can bind. A registration
/// geometry is tested against a `near` relation's `maxDistance` alone, and a
/// discovery carries no ordering terms, so no other fragment of the statement
/// can fail to render.
pub fn build_csr_query(query: &CompiledCsrQuery, window: &PageWindow) -> Result<CsrQuery, RenderError> {
    let condition = build_csr_conditions(query);
    let parts = query.geo.as_ref().map(geometry_parts).transpose()?.flatten();

    let mut select = registration_select();
    let count = match parts {
        None => {
            // clone: the count statement filters on the same condition
            select.cond_where(condition.clone());
            count_where(condition)
        }
        Some(GeometryParts { absent, present }) => {
            // clone: both parts narrow the same condition
            let absent = condition.clone().add(absent);
            let present = condition.add(present);
            let members = array_cat(part_page(&absent, window), part_page(&present, window));
            select.and_where(SeaExpr::col(Csr::Id).eq(any(members)));
            let mut count = Query::select();
            count.expr(scalar_subquery(count_where(absent)).add(scalar_subquery(count_where(present))));
            count.take()
        }
    };
    // Clause 5.5.9: the registrations are paged in identifier order in either
    // pagination mode. The identifier is unique, so an offset page is
    // deterministic across executions, and a cursor continues from the very
    // order its first page was read in.
    let read_order = apply_identifier_window(&mut select, SeaExpr::col(Csr::Id), window);

    Ok(CsrQuery {
        select: select.take(),
        count,
        read_order,
    })
}

/// The identifiers of the registrations `part` keeps that can fall on the page
/// `window` selects, as one array: the part read in identifier order through
/// the window widened to reach the page's end, so the page cut from the merged
/// parts is the page of their union.
fn part_page(part: &Condition, window: &PageWindow) -> SimpleExpr {
    let mut select = Query::select();
    // clone: both the page and the count read the part
    select.column(Csr::Id).from(Csr::Table).cond_where(part.clone());
    apply_identifier_window(&mut select, SeaExpr::col(Csr::Id), &window.through_page_end());
    array_of_subquery(select.take())
}

/// `SELECT count(*)` over the registrations `condition` keeps.
fn count_where(condition: Condition) -> SelectStatement {
    Query::select().expr(count_all()).from(Csr::Table).cond_where(condition).take()
}

/// The condition every registration on the answer meets: each filter of the
/// query except the geoquery, which splits the statement into two parts
/// instead of narrowing it.
fn build_csr_conditions(query: &CompiledCsrQuery) -> Condition {
    // Clause 5.9.2.4: an expired registration is deleted the moment its
    // `expiresAt` is reached, so discovery never reports one.
    let mut condition = Condition::all().add(not_expired());

    // Clause 5.12: the entity specification and the attribute names must match
    // the same `RegistrationInfo`, and the type and identifier the same
    // `EntityInfo`: one `registration_information` row decides every axis.
    let entities = ExpandedEntitySpecification {
        entity_type: query.entity_type.as_ref(),
        ids: &query.ids,
        id_pattern: query.id_pattern.as_ref(),
    };
    let axes = [
        row_matches(&entities),
        (!query.attrs.is_empty()).then(|| Condition::all().add(attribute_name_candidate(&query.attrs))),
    ];
    let row = axes.into_iter().flatten().fold(Condition::all(), Condition::add);
    if !row.is_empty() {
        condition = condition.add(some_information_row(row));
    }

    // Clause 4.9: the context source filter reads the registration's own
    // Context Source Properties.
    if let Some(expr) = &query.csf_expr {
        condition = condition.add(csr_expr_to_condition(expr));
    }

    // Clause 4.19: a registration's scopes are stored as the same `ltree`
    // label paths current entities carry, so a `scopeQ` compiles to the same
    // `lquery` match.
    if let Some(scope_q) = &query.scope_q {
        condition = condition.add(scope_condition(scope_q, &ScopeTarget::paths((Csr::Table, Csr::ScopePaths))));
    }

    if !query.encountered_sources.is_empty() {
        condition = condition.add(not_encountered(&query.encountered_sources));
    }

    condition = condition.add(temporal_partition(query.temporal.as_ref()));

    condition
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{
        context_source_pseudonym::ContextSourcePseudonym,
        identifier::Identifier,
        time::date_time::NgsiLdDateTime,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_query::{
        csr::compiled::CompiledCsrTemporalFilter,
        cursor::PageCursor,
        filter::parser,
        geo::query::GeoQuery,
        page_window::{PageStart, PageWindow},
        pattern::Pattern,
        scope::query::ScopeQuery,
        temporal::window::TimeInterval,
        type_selection::grammar::TypeExpr,
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
        s.parse::<NgsiLdDateTime>().expect("valid instant").get()
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
        for column in ["\"id\"", "\"registration_name\"", "\"context_source_alias\"", "\"additional_properties\""] {
            assert!(built.sql.contains(column), "missing {column}: {}", built.sql);
        }
        assert!(
            built.sql.contains("ST_AsEWKB(\"context_source_registrations\".\"location\") AS \"location\""),
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
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        let built = build(&q);
        assert!(
            built
                .sql
                .contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Vehicle','*']"),
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
        assert!(
            built.sql.contains("COALESCE(\"ri\".\"entity_id\", '') = ANY(ARRAY ['urn:a','urn:b',''])"),
            "{}",
            built.sql
        );
        assert!(built.sql.contains("\"ri\".\"entity_id\" = ANY(ARRAY ['urn:a','urn:b'])"), "{}", built.sql);
        assert!(!built.sql.contains("\"id\" = ANY("), "the registration id is not an entity id: {}", built.sql);
    }

    #[test]
    fn a_queried_id_pattern_is_evaluated_against_registered_ids_and_admits_registered_patterns() {
        let mut q = empty_query();
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        let built = build(&q);
        assert!(
            built
                .sql
                .contains("COALESCE(\"ri\".\"entity_id\", '') = ('') OR (\"ri\".\"entity_id\" ~ 'urn:ngsi-ld:.*')"),
            "{}",
            built.sql
        );
        assert!(!built.sql.contains("\"entity_id_pattern\" ="), "{}", built.sql);
    }

    /// Clause 5.12: the identifier pattern and the attribute names must both
    /// hold for the same `RegistrationInfo`, with or without a type selector,
    /// so the page and the count keep only rows meeting both.
    #[test]
    fn a_pattern_without_a_type_and_an_attribute_list_must_both_hold_on_the_row() {
        let mut q = empty_query();
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:Fleet.*").unwrap());
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        for statement in [&built.sql, &built.count_sql] {
            assert!(
                statement.contains(
                    "(COALESCE(\"ri\".\"entity_id\", '') = ('') OR (\"ri\".\"entity_id\" ~ 'urn:ngsi-ld:Fleet.*')) AND ((\"ri\".\"property_name_keys\" && "
                ),
                "{statement}"
            );
        }
    }

    /// Clause 5.12: an `EntityInfo` with neither `id` nor `idPattern` matches
    /// every identifier, and so does a row of an item without `entities`.
    #[test]
    fn an_entity_info_without_identifier_constraint_is_admitted_for_any_queried_id() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a"]);
        let built = build(&q);
        assert!(
            built
                .sql
                .contains("\"ri\".\"entity_id\" IS NULL AND (\"ri\".\"entity_id_pattern_form\" IS NULL OR"),
            "{}",
            built.sql
        );
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/temp")];
        let built = build(&q);
        assert!(built.sql.contains("\"property_name_keys\""), "{}", built.sql);
        assert!(built.sql.contains("\"relationship_name_keys\""), "{}", built.sql);
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
                .contains("OR (\"ri\".\"property_name_keys\" IS NULL AND \"ri\".\"relationship_name_keys\" IS NULL)"),
            "{}",
            built.sql
        );
    }

    /// Clause 5.12: the entity axis and the attribute axis must hold for the
    /// same `RegistrationInfo`, and the type and the identifier for the same
    /// `EntityInfo`; one row carries both, so one `EXISTS` decides them.
    #[test]
    fn entity_and_attribute_axes_are_decided_on_the_same_information_row() {
        let mut q = empty_query();
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert_eq!(built.sql.matches("FROM \"registration_information\" AS \"ri\"").count(), 1, "{}", built.sql);
        let row_scan = &built.sql[built.sql.find("FROM \"registration_information\" AS \"ri\"").expect("one row scan")..];
        let where_clause = &row_scan[row_scan.find(" WHERE ").expect("the scan has a WHERE")..];
        for predicate in [
            "\"ri\".\"registration_id\" = \"context_source_registrations\".\"id\"",
            "\"ri\".\"entity_types\"",
            "\"ri\".\"entity_id\" = ANY(",
            "\"ri\".\"property_name_keys\"",
            "\"ri\".\"relationship_name_keys\"",
        ] {
            assert!(where_clause.contains(predicate), "{predicate}: {}", built.sql);
        }
    }

    #[test]
    fn no_registration_statement_walks_json() {
        let mut q = empty_query();
        q.entity_type = Some(TypeExpr::And(vec![
            TypeExpr::Type(iri("https://example.com/Vehicle")),
            TypeExpr::Type(iri("https://example.com/Boat")),
        ]));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        q.attrs = vec![iri("https://example.com/speed")];
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        q.scope_q = Some(ScopeQuery::parse("/Madrid/#").unwrap());
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .interval(TimeInterval::Before(dt("2024-01-01T00:00:00Z")))
                .timeproperty(None)
                .build(),
        );
        let mut pattern_query = empty_query();
        pattern_query.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        for built in [build(&q), build(&pattern_query)] {
            for statement in [&built.sql, &built.count_sql] {
                for json_walk in [
                    "jsonb_array_elements",
                    "->",
                    "jsonb_exists",
                    "\"information\"",
                    "\"observation_interval\"",
                    "\"management_interval\"",
                ] {
                    assert!(!statement.contains(json_walk), "{json_walk}: {statement}");
                }
            }
        }
    }

    #[test]
    fn every_selective_axis_carries_its_indexed_test() {
        let mut q = empty_query();
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert!(
            built
                .sql
                .contains("COALESCE(\"ri\".\"entity_types\", '{*}') && ARRAY ['https://example.com/Vehicle','*']"),
            "{}",
            built.sql
        );
        assert!(
            built
                .sql
                .contains("COALESCE(\"ri\".\"entity_id\", '') = ANY(ARRAY ['urn:ngsi-ld:Vehicle:1','']) OR \"ri\".\"entity_id_pattern\" IS NOT NULL"),
            "{}",
            built.sql
        );
        assert!(built.sql.contains("\"ri\".\"property_name_keys\" && (ARRAY (SELECT"), "{}", built.sql);
        assert!(!built.sql.contains("NOT EXISTS"), "{}", built.sql);
    }

    #[test]
    fn a_query_without_selective_filters_carries_no_preselect() {
        // Nothing to narrow: an unfiltered discovery must not pay for a
        // semi-join that would admit every registration anyway; the rows it
        // reads are those the projection carries.
        let built = build(&empty_query());
        assert!(!built.sql.contains("AS \"ri\""), "{}", built.sql);
        assert!(!built.sql.contains("EXISTS("), "{}", built.sql);
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
        // Clause 5.10.2.4: a registration with no location still matches, and
        // one whose location reaches into the reference possibly contains a
        // location within it.
        assert!(built.sql.contains("\"location\" IS NULL"), "{}", built.sql);
        assert!(built.sql.contains(" _ST_Intersects("), "{}", built.sql);
    }

    /// Each part of the page is read through the window widened to reach the
    /// page's end, so every registration of the page is among what its part
    /// returns, and the page is cut once more from the merged parts.
    #[test]
    fn a_geo_discovery_pages_the_two_parts_and_cuts_the_page_from_their_union() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        let built = build_paged(&q, &PageWindow::offset(10, 30));
        assert!(
            built
                .sql
                .contains(r#""id" = ANY(array_cat(ARRAY (SELECT "id" FROM "context_source_registrations" WHERE "#),
            "{}",
            built.sql
        );
        assert_eq!(built.sql.matches(r#"ORDER BY "id" ASC LIMIT 40 OFFSET 0)"#).count(), 2, "{}", built.sql);
        assert!(built.sql.ends_with(r#"ORDER BY "id" ASC LIMIT 10 OFFSET 30"#), "{}", built.sql);
        assert!(!built.sql.contains(r#""location" IS NULL OR ST_"#), "{}", built.sql);
    }

    #[test]
    fn a_geo_discovery_cursor_page_bounds_each_part_by_the_cursor() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        let window = PageWindow::after(5, "urn:ngsi-ld:ContextSourceRegistration:7".parse().expect("valid URI"));
        let built = build_paged(&q, &window);
        assert_eq!(
            built.sql.matches(r#""id" > 'urn:ngsi-ld:ContextSourceRegistration:7'"#).count(),
            3,
            "both parts and the page: {}",
            built.sql
        );
        assert_eq!(built.sql.matches(r#"ORDER BY "id" ASC LIMIT 5"#).count(), 3, "{}", built.sql);
    }

    /// The parts are disjoint, so the total is their sum, and each part keeps
    /// every other filter of the query.
    #[test]
    fn a_geo_discovery_counts_the_sum_of_the_two_parts() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        let built = build(&q);
        assert!(
            built
                .count_sql
                .starts_with(r#"SELECT (SELECT COUNT(*) FROM "context_source_registrations" WHERE "#),
            "{}",
            built.count_sql
        );
        assert!(built.count_sql.contains(r#"AND "location" IS NULL) + (SELECT COUNT(*)"#), "{}", built.count_sql);
        assert!(
            built
                .count_sql
                .contains(r#""location" IS NOT NULL AND ((COALESCE("location_reach", "location_valid", "location") && ST_MakeValid(ST_GeomFromEWKB("#),
            "{}",
            built.count_sql
        );
        assert!(built.count_sql.contains(" AND _ST_Intersects("), "{}", built.count_sql);
        assert_eq!(built.count_sql.matches("registration_information").count(), 2, "{}", built.count_sql);
    }

    /// A geoquery whose reduction constrains no geometry keeps every
    /// registration, so the statement is not split.
    #[test]
    fn a_minimum_distance_only_geo_discovery_is_not_split() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: None,
                min_distance: Some("100".parse().expect("positive distance")),
            },
            ..geo_query("https://uri.etsi.org/ngsi-ld/location")
        });
        let built = build(&q);
        assert!(!built.sql.contains("array_cat"), "{}", built.sql);
        assert!(!built.count_sql.contains('+'), "{}", built.count_sql);
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
            built
                .sql
                .contains("\"observation_interval_start\" IS NULL AND \"management_interval_start\" IS NULL"),
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
        assert!(built.sql.contains("\"observation_interval_start\" IS NOT NULL"), "{}", built.sql);
        assert!(built.sql.contains("\"observation_interval_start\" <="), "{}", built.sql);
        assert!(!built.sql.contains("management_interval_start\" IS NULL"), "{}", built.sql);
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
        assert!(built.sql.contains("\"management_interval_start\" IS NOT NULL"), "{}", built.sql);
        assert!(built.sql.contains("\"management_interval_end\" >="), "{}", built.sql);
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
        assert!(built.sql.contains("\"observation_interval_start\" <="), "{}", built.sql);
        assert!(built.sql.contains("\"observation_interval_end\" >="), "{}", built.sql);
    }

    /// Clause 4.9: `csf` reads the stored Context Source Property document,
    /// which holds the Table 5.2.9-1 members beside the added properties, never
    /// the added properties alone.
    #[test]
    fn csf_reads_the_context_source_properties_column() {
        let mut q = empty_query();
        q.csf_expr = Some(parser::parse("\"https://uri.etsi.org/ngsi-ld/mode\"==\"exclusive\"").unwrap());
        let built = build(&q);
        assert!(built.sql.contains("\"context_source_properties\" -> ("), "{}", built.sql);
        let filtered = built.sql.split(" WHERE ").nth(1).expect("a WHERE clause");
        assert!(!filtered.contains("additional_properties"), "{}", built.sql);
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

    /// Clause 5.12: a registration whose alias the request already passed
    /// through never matches.
    #[test]
    fn build_with_encountered_sources() {
        let mut q = empty_query();
        q.encountered_sources = vec![ContextSourcePseudonym::new("broker-b").unwrap()];
        let built = build(&q);
        assert!(built.sql.contains("\"context_source_alias\" <> ALL(ARRAY ['broker-b'])"), "{}", built.sql);
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
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
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
        assert!(built.sql.contains(" _ST_Intersects("), "{}", built.sql);
        assert!(built.sql.contains("ST_GeomFromEWKB"), "{}", built.sql);
    }
}
