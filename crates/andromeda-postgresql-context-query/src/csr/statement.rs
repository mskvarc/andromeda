//! SQL generation for Context Source Registration discovery (clause 5.10.2).

use crate::csr::{
    encountered_sources::not_encountered,
    entity_specification::row_matches,
    expiry_predicate::not_expired,
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
    q_predicate::document::condition::document_condition,
    scope_predicate::{ScopeTarget, condition as scope_condition},
};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    column_reference::ColumnReference,
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
            select.cond_where(condition.clone());
            count_where(condition)
        }
        Some(GeometryParts { absent, present }) => {
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
    if let Some(csf) = &query.csf {
        condition = condition.add(document_condition(csf, &ColumnReference::bare(Csr::ContextSourceProperties)));
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
mod tests {
    use crate::csr::statement::{CsrQuery, build_csr_query};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{
        context_source_pseudonym::ContextSourcePseudonym,
        identifier::Identifier,
        time::date_time::NgsiLdDateTime,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_query::{
        csr::compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
        cursor::PageCursor,
        filter::{language_matching::LanguageMatching, parser, reading::filter_reading::FilterReading},
        geo::query::GeoQuery,
        page_window::{PageStart, PageWindow},
        pattern::Pattern,
        scope::query::ScopeQuery,
        temporal::window::TimeInterval,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;

    fn build(query: &CompiledCsrQuery) -> CsrQuery {
        build_paged(query, &PageWindow::offset(20, 0))
    }

    fn build_paged(query: &CompiledCsrQuery, window: &PageWindow) -> CsrQuery {
        build_csr_query(query, window).expect("a renderable discovery")
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
        assert_statement_snapshot!("statement-build-empty-query-returns-base-sql", build(&empty_query()).select);
    }

    /// Clause 5.9.2.4: a registration past its `expiresAt` is gone from
    /// discovery whether or not the sweeper has deleted its row yet.
    #[test]
    fn every_discovery_excludes_expired_registrations_from_the_page_and_the_count() {
        let built = build(&empty_query());
        assert_statement_snapshot!(
            "statement-every-discovery-excludes-expired-registrations-from-the-page-and-the-count-1",
            built.select
        );
        assert_statement_snapshot!(
            "statement-every-discovery-excludes-expired-registrations-from-the-page-and-the-count-2",
            built.count
        );
    }

    #[test]
    fn discovery_reads_the_shared_registration_projection() {
        assert_statement_snapshot!("statement-discovery-reads-the-shared-registration-projection", build(&empty_query()).select);
    }

    /// Clause 5.5.9: an offset page reads the registrations in the unique
    /// identifier order, so it is deterministic across executions.
    #[test]
    fn an_offset_discovery_orders_by_id() {
        let built = build_paged(&empty_query(), &PageWindow::offset(10, 30));
        assert_statement_snapshot!("statement-an-offset-discovery-orders-by-id", built.select);
    }

    /// The first page of a cursor walk reads in the identifier order its
    /// cursors continue from, never by `created_at`.
    #[test]
    fn a_fresh_discovery_orders_by_id() {
        let window = PageWindow {
            limit: 3,
            start: PageStart::Cursor(None),
        };
        assert_statement_snapshot!("statement-a-fresh-discovery-orders-by-id", build_paged(&empty_query(), &window).select);
    }

    /// Clause 5.5.9.1: a forward cursor pages `id > boundary` ascending over the
    /// registration id, replacing the offset window.
    #[test]
    fn a_forward_cursor_pages_id_ascending_past_the_boundary() {
        let built = build_paged(&empty_query(), &PageWindow::after(20, "urn:ngsi-ld:Csr:010".parse().expect("valid URI")));
        assert_statement_snapshot!("statement-a-forward-cursor-pages-id-ascending-past-the-boundary", built.select);
    }

    /// Clause 5.5.9.1: a backward cursor pages `id < boundary` descending (the
    /// store reverses the rows to ascending for display).
    #[test]
    fn a_backward_cursor_pages_id_descending_before_the_boundary() {
        let window = PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(PageCursor::Before("urn:ngsi-ld:Csr:010".parse().expect("valid URI")))),
        };
        assert_statement_snapshot!(
            "statement-a-backward-cursor-pages-id-descending-before-the-boundary",
            build_paged(&empty_query(), &window).select
        );
    }

    /// Clause 5.5.9.1: the from-end cursor pages the last ids descending with no
    /// boundary predicate.
    #[test]
    fn a_from_end_cursor_pages_the_last_ids_descending() {
        let window = PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(PageCursor::End)),
        };
        assert_statement_snapshot!(
            "statement-a-from-end-cursor-pages-the-last-ids-descending",
            build_paged(&empty_query(), &window).select
        );
    }

    #[test]
    fn build_with_entity_type() {
        let mut q = empty_query();
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        assert_statement_snapshot!("statement-build-with-entity-type", build(&q).select);
    }

    /// Clause 5.12: entity identifiers are matched against the registered
    /// `EntityInfo` entries, never against the registration's own identifier.
    #[test]
    fn entity_ids_are_matched_against_entity_info_not_the_registration_id() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a", "urn:b"]);
        assert_statement_snapshot!("statement-entity-ids-are-matched-against-entity-info-not-the-registration-id", build(&q).select);
    }

    #[test]
    fn a_queried_id_pattern_is_evaluated_against_registered_ids_and_admits_registered_patterns() {
        let mut q = empty_query();
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        assert_statement_snapshot!(
            "statement-a-queried-id-pattern-is-evaluated-against-registered-ids-and-admits-registered-patterns",
            build(&q).select
        );
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
        assert_statement_snapshot!(
            "statement-a-pattern-without-a-type-and-an-attribute-list-must-both-hold-on-the-row-1",
            built.select
        );
        assert_statement_snapshot!(
            "statement-a-pattern-without-a-type-and-an-attribute-list-must-both-hold-on-the-row-2",
            built.count
        );
    }

    /// Clause 5.12: an `EntityInfo` with neither `id` nor `idPattern` matches
    /// every identifier, and so does a row of an item without `entities`.
    #[test]
    fn an_entity_info_without_identifier_constraint_is_admitted_for_any_queried_id() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a"]);
        assert_statement_snapshot!(
            "statement-an-entity-info-without-identifier-constraint-is-admitted-for-any-queried-id",
            build(&q).select
        );
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/temp")];
        assert_statement_snapshot!("statement-build-with-attrs", build(&q).select);
    }

    /// Clause 5.12: an information item naming no property and no relationship
    /// registered entities only, and matches any attribute list.
    #[test]
    fn an_entity_only_information_item_matches_any_attribute_list() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed")];
        assert_statement_snapshot!("statement-an-entity-only-information-item-matches-any-attribute-list", build(&q).select);
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
        assert_statement_snapshot!("statement-entity-and-attribute-axes-are-decided-on-the-same-information-row", build(&q).select);
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
        let filtered = build(&q);
        let patterned = build(&pattern_query);
        assert_statement_snapshot!("statement-no-registration-statement-walks-json-1", filtered.select);
        assert_statement_snapshot!("statement-no-registration-statement-walks-json-2", filtered.count);
        assert_statement_snapshot!("statement-no-registration-statement-walks-json-3", patterned.select);
        assert_statement_snapshot!("statement-no-registration-statement-walks-json-4", patterned.count);
    }

    #[test]
    fn every_selective_axis_carries_its_indexed_test() {
        let mut q = empty_query();
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        q.attrs = vec![iri("https://example.com/speed")];
        assert_statement_snapshot!("statement-every-selective-axis-carries-its-indexed-test", build(&q).select);
    }

    #[test]
    fn a_query_without_selective_filters_carries_no_preselect() {
        // Nothing to narrow: an unfiltered discovery must not pay for a
        // semi-join that would admit every registration anyway; the rows it
        // reads are those the projection carries.
        assert_statement_snapshot!("statement-a-query-without-selective-filters-carries-no-preselect", build(&empty_query()).select);
    }

    #[test]
    fn the_count_statement_carries_the_same_preselect_as_the_page() {
        // Clause 5.5.9: the total must count exactly the rows the page draws
        // from, so both statements share one condition.
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed")];
        assert_statement_snapshot!("statement-the-count-statement-carries-the-same-preselect-as-the-page", build(&q).count);
    }

    #[test]
    fn build_with_geo_within() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        // Clause 5.10.2.4: a registration with no location still matches, and
        // one whose location reaches into the reference possibly contains a
        // location within it.
        assert_statement_snapshot!("statement-build-with-geo-within", build(&q).select);
    }

    /// Each part of the page is read through the window widened to reach the
    /// page's end, so every registration of the page is among what its part
    /// returns, and the page is cut once more from the merged parts.
    #[test]
    fn a_geo_discovery_pages_the_two_parts_and_cuts_the_page_from_their_union() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        assert_statement_snapshot!(
            "statement-a-geo-discovery-pages-the-two-parts-and-cuts-the-page-from-their-union",
            build_paged(&q, &PageWindow::offset(10, 30)).select
        );
    }

    #[test]
    fn a_geo_discovery_cursor_page_bounds_each_part_by_the_cursor() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        let window = PageWindow::after(5, "urn:ngsi-ld:ContextSourceRegistration:7".parse().expect("valid URI"));
        assert_statement_snapshot!(
            "statement-a-geo-discovery-cursor-page-bounds-each-part-by-the-cursor",
            build_paged(&q, &window).select
        );
    }

    /// The parts are disjoint, so the total is their sum, and each part keeps
    /// every other filter of the query.
    #[test]
    fn a_geo_discovery_counts_the_sum_of_the_two_parts() {
        let mut q = empty_query();
        q.geo = Some(geo_query("https://uri.etsi.org/ngsi-ld/location"));
        q.ids = ids(&["urn:ngsi-ld:Vehicle:1"]);
        assert_statement_snapshot!("statement-a-geo-discovery-counts-the-sum-of-the-two-parts", build(&q).count);
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
        assert_statement_snapshot!("statement-a-minimum-distance-only-geo-discovery-is-not-split-1", built.select);
        assert_statement_snapshot!("statement-a-minimum-distance-only-geo-discovery-is-not-split-2", built.count);
    }

    /// Table 5.2.9-1: `observationSpace` and `operationSpace` are geometries of
    /// their own, reached when the geoquery names them; any other `GeoProperty`,
    /// including the clause 5.10.2.4 default, reads `location`.
    #[test]
    fn the_geoproperty_selects_the_registration_geometry() {
        for (index, geoproperty) in [
            "https://uri.etsi.org/ngsi-ld/location",
            "https://uri.etsi.org/ngsi-ld/observationSpace",
            "https://uri.etsi.org/ngsi-ld/operationSpace",
            "https://example.com/somewhere",
        ]
        .into_iter()
        .enumerate()
        {
            let mut q = empty_query();
            q.geo = Some(geo_query(geoproperty));
            assert_statement_snapshot!(
                format!("statement-the-geoproperty-selects-the-registration-geometry-{}", index + 1),
                build(&q).select
            );
        }
    }

    /// Clause 5.10.2.4: without a temporal query only registrations without
    /// any interval are considered.
    #[test]
    fn no_temporal_query_admits_only_latest_information_registrations() {
        assert_statement_snapshot!(
            "statement-no-temporal-query-admits-only-latest-information-registrations",
            build(&empty_query()).select
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
        assert_statement_snapshot!("statement-build-with-temporal-before", build(&q).select);
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
        assert_statement_snapshot!("statement-build-with-temporal-after", build(&q).select);
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
        assert_statement_snapshot!("statement-build-with-temporal-between", build(&q).select);
    }

    /// Clause 4.9: `csf` reads the stored Context Source Property document,
    /// which holds the Table 5.2.9-1 members beside the added properties, never
    /// the added properties alone.
    #[test]
    fn csf_reads_the_context_source_properties_column() {
        let mut q = empty_query();
        q.csf = Some(FilterReading::of(
            &parser::parse("\"https://uri.etsi.org/ngsi-ld/mode\"==\"exclusive\"").unwrap(),
            LanguageMatching::TagEquality,
        ));
        assert_statement_snapshot!("statement-csf-reads-the-context-source-properties-column", build(&q).select);
    }

    #[test]
    fn a_scope_query_matches_the_stored_label_paths_with_an_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::parse("/Madrid/#").unwrap());
        assert_statement_snapshot!("statement-a-scope-query-matches-the-stored-label-paths-with-an-lquery", build(&q).select);
    }

    /// Clause 5.12: a registration whose alias the request already passed
    /// through never matches.
    #[test]
    fn build_with_encountered_sources() {
        let mut q = empty_query();
        q.encountered_sources = vec![ContextSourcePseudonym::new("broker-b").unwrap()];
        assert_statement_snapshot!("statement-build-with-encountered-sources", build(&q).select);
    }

    #[test]
    fn build_count_sql_always_present() {
        assert_statement_snapshot!("statement-build-count-sql-always-present", build(&empty_query()).count);
    }

    #[test]
    fn build_with_pagination() {
        assert_statement_snapshot!(
            "statement-build-with-pagination",
            build_paged(&empty_query(), &PageWindow::offset(50, 100)).select
        );
    }

    #[test]
    fn build_with_multiple_filters() {
        let mut q = empty_query();
        q.entity_type = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        q.ids = ids(&["urn:a"]);
        q.attrs = vec![iri("https://example.com/speed")];
        assert_statement_snapshot!("statement-build-with-multiple-filters", build(&q).select);
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
        assert_statement_snapshot!("statement-a-polygon-reference-geometry-renders-the-same-relation-as-a-point", build(&q).select);
    }
}
