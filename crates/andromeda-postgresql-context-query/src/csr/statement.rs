//! SQL generation for Context Source Registration queries using sea-query.

use crate::csr::{
    coverage::entity_type_coverage,
    filter::csr_expr_to_condition,
    projection::{attribute_name_candidate, id_pattern_candidate},
    unnesting::{ent, entities_of_item, info, information_covers},
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, temporal::window::TimeInterval};
use andromeda_postgresql_query::{
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::GeometrySource,
    q_predicate::attribute_names::ResolvedAttributeNames,
};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::{cast, cast_compound},
    jsonb::{jsonb_array_elements, jsonb_array_elements_text},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    postgis::geometry_as_geojson,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, Func, Order, Query, SelectStatement, SimpleExpr};

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
}

/// Builds a parameterized SQL query to discover Context Source Registrations.
///
/// Generates filter conditions for entity type, IDs, ID patterns, attributes,
/// `q` expressions, geo location, and temporal intervals. CSR-specific semantics
/// apply: registrations without a location or temporal interval still match
/// geo/temporal filters (per NGSI-LD 5.10.2).
pub fn build_csr_query(query: &CompiledCsrQuery) -> Result<CsrQuery, RenderError> {
    let condition = build_csr_conditions(query)?;

    let mut select = Query::select();
    select
        .column(Csr::Id)
        .column(Csr::Type)
        .column(Csr::RegistrationName)
        .column(Csr::Description)
        .column(Csr::Information)
        .column(Csr::Tenant)
        .column(Csr::ObservationInterval)
        .column(Csr::ManagementInterval)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::Location).into()), Csr::Location)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::ObservationSpace).into()), Csr::ObservationSpace)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::OperationSpace).into()), Csr::OperationSpace)
        .column(Csr::ExpiresAt)
        .column(Csr::Endpoint)
        .column(Csr::Accept)
        .expr_as(cast(SeaExpr::col(Csr::Mode).into(), PostgresType::Text), Csr::Mode)
        .column(Csr::Operations)
        .column(Csr::Scope)
        .column(Csr::RefreshRate)
        .column(Csr::Management)
        .column(Csr::ContextSourceInfo)
        .column(Csr::JsonldContext)
        .column(Csr::ContextSourceAlias)
        .expr_as(cast(SeaExpr::col(Csr::Status).into(), PostgresType::Text), Csr::Status)
        .column(Csr::TimesSent)
        .column(Csr::TimesFailed)
        .column(Csr::LastSuccess)
        .column(Csr::LastFailure)
        .column(Csr::CreatedAt)
        .column(Csr::ModifiedAt)
        .column(Csr::AdditionalProperties)
        .from(Csr::Table)
        // clone: the count statement filters on the same condition
        .cond_where(condition.clone())
        // `id` breaks ties on `created_at`: two registrations minted in the same
        // microsecond would otherwise straddle a page boundary inconsistently
        // across executions (clause 5.5.9 requires a stable order).
        .order_by(Csr::CreatedAt, Order::Asc)
        .order_by(Csr::Id, Order::Asc)
        .limit(query.pagination.limit as u64)
        .offset(query.pagination.offset as u64);

    // Count query always generated for CSR
    let mut count = Query::select();
    count.expr(count_all()).from(Csr::Table).cond_where(condition);

    Ok(CsrQuery {
        select: select.take(),
        count: count.take(),
    })
}

/// Builds sea-query Condition from compiled CSR query filters.
fn build_csr_conditions(query: &CompiledCsrQuery) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Entity type filter: match against information[].entities[].type. Shared
    // with the clause-5.10.2 forwarding match so the two read a type
    // registration the same way.
    if let Some(entity_type) = &query.entity_type {
        condition = condition.add(entity_type_coverage(entity_type));
    }

    // ID filter
    if !query.ids.is_empty() {
        let ids: Vec<&str> = query.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col(Csr::Id).is_in(ids));
    }

    // ID pattern filter: match against information[].entities[].idPattern. The
    // projection preselect compares the stored pattern text and never evaluates
    // it, so the unnesting below remains what decides the match.
    if let Some(id_pattern) = &query.id_pattern {
        condition = condition.add(id_pattern_candidate(id_pattern));
        // Discovery advertises a stored `idPattern` by comparing its text, so the
        // entry-level predicate is an exact match, not a regex evaluation.
        let mut entities_match = entities_of_item();
        entities_match.and_where(SeaExpr::expr(ent().json_get_text(text_literal("idPattern"))).eq(id_pattern.as_str()));
        condition = condition.add(information_covers(entities_match));
    }

    // Attrs filter: match against information[].propertyNames /
    // relationshipNames. The preselect resolves the requested names through the
    // dictionary the projection interns them with; the unnesting decides.
    if !query.attrs.is_empty() {
        condition = condition.add(attribute_name_candidate(&query.attrs));
        let names: Vec<String> = query.attrs.iter().map(|iri| iri.as_str().to_string()).collect();
        condition = condition.add(attribute_names_declared(&names));
    }

    // Q expression filter against additional_properties JSONB column
    if let Some(expr) = &query.q_expr {
        condition = condition.add(csr_expr_to_condition(expr)?);
    }

    // Geo filter: a registration that declares no location still matches
    // (clause 5.10.2).
    if let Some(geo) = &query.geo {
        condition = condition.add(geo_sql::condition(
            geo,
            GeometrySource::Registration { column: "location" },
            &ResolvedAttributeNames::default(),
        )?);
    }

    // Temporal filter: CSRs without the relevant interval still match (per spec)
    if let Some(temporal) = &query.temporal {
        let timeprop = temporal.timeproperty.unwrap_or(TimeProp::Observed);
        let interval_col = match timeprop {
            TimeProp::Created | TimeProp::Modified => Csr::ManagementInterval,
            TimeProp::Observed | TimeProp::Deleted => Csr::ObservationInterval,
        };

        // Clause 5.10.2: a registration that declares no interval may still
        // serve the window, which is why every bounded arm below disjoins
        // `IS NULL` rather than requiring the column to be present.
        let missing_interval = || SeaExpr::expr(SeaExpr::col(interval_col)).is_null();
        match temporal.interval {
            // An unbounded filter narrows nothing, so it emits no predicate at
            // all. Discovery cannot reach this today — a temporal filter is
            // only built when `timerel` and `timeAt` are both present — but the
            // interval type admits it and the arm says what it would mean.
            TimeInterval::Unbounded => {}
            TimeInterval::Before(instant) => {
                condition = condition.add(
                    Condition::any()
                        .add(missing_interval())
                        .add(SeaExpr::expr(interval_bound(interval_col, "startAt")).lt(interval_instant(instant.to_rfc3339()))),
                );
            }
            TimeInterval::After(instant) => {
                condition = condition.add(
                    Condition::any()
                        .add(missing_interval())
                        .add(SeaExpr::expr(interval_bound(interval_col, "endAt")).gt(interval_instant(instant.to_rfc3339()))),
                );
            }
            TimeInterval::Between { from, to } => {
                condition = condition.add(
                    Condition::any().add(missing_interval()).add(
                        Condition::all()
                            .add(SeaExpr::expr(interval_bound(interval_col, "startAt")).lte(interval_instant(to.to_rfc3339())))
                            .add(SeaExpr::expr(interval_bound(interval_col, "endAt")).gte(interval_instant(from.to_rfc3339()))),
                    ),
                );
            }
        }
    }

    Ok(condition)
}

/// One bound of a registration's stored interval, lifted to `timestamptz`:
/// `("interval_col"->>'member')::timestamptz`.
fn interval_bound(interval_col: Csr, member: &str) -> SimpleExpr {
    cast_compound(
        SimpleExpr::from(SeaExpr::col(interval_col)).json_get_text(text_literal(member)),
        PostgresType::Timestamptz,
    )
}

/// A filter instant bound as RFC 3339 text and cast in the statement, so it
/// compares as `timestamptz` against a lifted interval bound.
fn interval_instant(rfc3339: String) -> SimpleExpr {
    cast(SeaExpr::val(rfc3339).into(), PostgresType::Timestamptz)
}

/// Whether some `information` item declares one of `names` as a property or a
/// relationship name.
///
/// Each information item's `propertyNames` and `relationshipNames` arrays are
/// unnested and tested for membership in the requested set; a missing array is
/// coalesced to an empty one so an item that names nothing on an axis is simply
/// no match rather than a NULL that aborts the test.
fn attribute_names_declared(names: &[String]) -> SimpleExpr {
    let member_of = |member: &'static str, alias: &'static str| {
        let coalesced = Func::coalesce([info().json_get(text_literal(member)), cast(text_literal("[]"), PostgresType::Jsonb)]);
        let mut scan = Query::select();
        scan.expr(int_literal(1))
            .from_function(jsonb_array_elements_text(coalesced.into()), Alias::new(alias))
            .and_where(SeaExpr::expr(SeaExpr::col(Alias::new(alias))).is_in(names.to_vec()));
        SeaExpr::exists(scan)
    };
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_array_elements(SeaExpr::col(Csr::Information).into()), Alias::new("info"))
        .cond_where(Condition::any().add(member_of("propertyNames", "pn")).add(member_of("relationshipNames", "rn")));
    SeaExpr::exists(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrTemporalFilter, filter::parser, geo::query::GeoQuery, pattern::Pattern};
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use sea_query::PostgresQueryBuilder;

    /// A built CSR discovery rendered with its values inlined: the paginated
    /// statement and its always-present count.
    struct Built {
        sql: String,
        count_sql: String,
    }

    fn build(query: &CompiledCsrQuery) -> Built {
        let csr = build_csr_query(query).unwrap();
        Built {
            sql: csr.select.to_string(PostgresQueryBuilder),
            count_sql: csr.count.to_string(PostgresQueryBuilder),
        }
    }

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn dt(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// A GeoJSON point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
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

    #[test]
    fn the_projection_names_every_column_quoted_and_casts_geometry_and_enum_columns() {
        // The `Iden` enum must render byte-identically to the hand-quoted column
        // list it replaced: plain columns quoted, geometry columns as GeoJSON,
        // and the enum columns cast to text, each with the same output alias.
        let built = build(&empty_query());
        for column in ["\"id\"", "\"type\"", "\"registration_name\"", "\"jsonld_context\"", "\"additional_properties\""] {
            assert!(built.sql.contains(column), "missing {column}: {}", built.sql);
        }
        assert!(built.sql.contains("ST_AsGeoJSON(\"location\")::jsonb AS \"location\""), "{}", built.sql);
        assert!(
            built.sql.contains("ST_AsGeoJSON(\"observation_space\")::jsonb AS \"observation_space\""),
            "{}",
            built.sql
        );
        assert!(built.sql.contains("\"mode\"::text AS \"mode\""), "{}", built.sql);
        assert!(built.sql.contains("\"status\"::text AS \"status\""), "{}", built.sql);
    }

    #[test]
    fn build_orders_by_created_at_then_id_for_stable_pagination() {
        // Clause 5.5.9: two registrations minted in the same microsecond must
        // not straddle a page boundary inconsistently across executions, so the
        // `created_at` order is broken by the `id` tiebreaker.
        let q = empty_query();
        let built = build(&q);
        assert!(built.sql.contains("ORDER BY \"created_at\" ASC, \"id\" ASC"), "{}", built.sql);
    }

    #[test]
    fn build_with_entity_type() {
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        let built = build(&q);
        assert!(built.sql.contains("'type'"), "{}", built.sql);
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a", "urn:b"]);
        let built = build(&q);
        assert!(built.sql.contains("\"id\" IN"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        let built = build(&q);
        assert!(built.sql.contains("'idPattern'"), "{}", built.sql);
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/temp")];
        let built = build(&q);
        assert!(built.sql.contains("propertyNames"));
        assert!(built.sql.contains("relationshipNames"));
    }

    #[test]
    fn the_selective_filters_preselect_against_the_routing_projection() {
        // Type, id pattern and attribute names each gain an indexed semi-join
        // over the projection; the `information` unnesting they are conjoined
        // with still decides membership, so the preselect only has to be a
        // superset of it.
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build(&q);
        assert!(built.sql.contains("\"ri\".\"entity_types\" IS NULL"), "{}", built.sql);
        assert!(built.sql.contains("\"ri\".\"entity_id_pattern\" IS NULL"), "{}", built.sql);
        assert!(built.sql.contains("\"ri\".\"property_name_ids\" IS NULL"), "{}", built.sql);
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
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let built = build(&q);
        // Clause 5.10.2: a registration with no location still matches.
        assert!(built.sql.contains("\"location\" IS NULL"), "{}", built.sql);
        assert!(built.sql.contains("ST_Within"), "{}", built.sql);
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
        assert!(built.sql.contains("observation_interval\" IS NULL"));
        assert!(built.sql.contains("startAt"));
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
        assert!(built.sql.contains("management_interval\" IS NULL"));
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

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(parser::parse("temperature").unwrap());
        let built = build(&q);
        assert!(built.sql.contains("\"additional_properties\" ?"));
    }

    #[test]
    fn build_count_sql_always_present() {
        let q = empty_query();
        let built = build(&q);
        assert!(built.count_sql.contains("COUNT(*)"), "{}", built.count_sql);
    }

    #[test]
    fn build_with_pagination() {
        let mut q = empty_query();
        q.pagination.limit = 50;
        q.pagination.offset = 100;
        let built = build(&q);
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
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
            geometry: Geometry::from_geojson_value(&ring).expect("polygon parses"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let built = build(&q);
        assert!(built.sql.contains("ST_Within"), "{}", built.sql);
        assert!(built.sql.contains("ST_GeomFromEWKB"), "{}", built.sql);
    }
}
