//! SQL generation for Context Source Registration queries using sea-query.

use crate::csr::filter::csr_expr_to_condition;
use andromeda_domain::temporal::{TimeProp, TimeRel};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, error::QueryError};
use andromeda_postgresql_query::{
    bind::sea_values_to_bind_values,
    built::BuiltQuery,
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::{GeometrySource, NullGeometryPolicy},
};
use sea_query::{Alias, Condition, Expr as SeaExpr, Order, PostgresQueryBuilder, Query};

/// Every column a registration row projects, with geometries rendered as GeoJSON.
const CSR_COLUMNS: &str = concat!(
    "\"id\", \"type\", \"registration_name\", \"description\", \"information\", \"tenant\", ",
    "\"observation_interval\", \"management_interval\", ",
    "ST_AsGeoJSON(\"location\")::JSONB AS \"location\", ",
    "ST_AsGeoJSON(\"observation_space\")::JSONB AS \"observation_space\", ",
    "ST_AsGeoJSON(\"operation_space\")::JSONB AS \"operation_space\", ",
    "\"expires_at\", \"endpoint\", \"accept\", \"mode\"::text AS \"mode\", \"operations\", \"scope\", ",
    "\"refresh_rate\", \"management\", \"context_source_info\", \"jsonld_context\", \"context_source_alias\", ",
    "\"status\"::text AS \"status\", \"times_sent\", \"times_failed\", \"last_success\", \"last_failure\", ",
    "\"created_at\", \"modified_at\", \"additional_properties\""
);

/// Builds a parameterized SQL query to discover Context Source Registrations.
///
/// Generates filter conditions for entity type, IDs, ID patterns, attributes,
/// `q` expressions, geo location, and temporal intervals. CSR-specific semantics
/// apply: registrations without a location or temporal interval still match
/// geo/temporal filters (per NGSI-LD 5.10.2).
pub fn build_csr_query(query: &CompiledCsrQuery) -> Result<BuiltQuery<'static>, RenderError> {
    let condition = build_csr_conditions(query)?;

    let mut select = Query::select();
    select
        .expr(SeaExpr::cust(CSR_COLUMNS))
        .from(Alias::new("context_source_registrations"))
        // clone: the count statement filters on the same condition
        .cond_where(condition.clone())
        .order_by(Alias::new("created_at"), Order::Asc)
        .limit(query.pagination.limit as u64)
        .offset(query.pagination.offset as u64);

    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    // Count query always generated for CSR
    let mut count_select = Query::select();
    count_select
        .expr(SeaExpr::cust("COUNT(*)"))
        .from(Alias::new("context_source_registrations"))
        .cond_where(condition);
    let (count_sql, _) = count_select.build(PostgresQueryBuilder);

    Ok(BuiltQuery {
        sql,
        count_sql: Some(count_sql),
        bind_values,
    })
}

/// Builds sea-query Condition from compiled CSR query filters.
fn build_csr_conditions(query: &CompiledCsrQuery) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Entity type filter: match against information[].entities[].type
    if let Some(entity_type) = &query.entity_type {
        condition = condition.add(SeaExpr::cust_with_values(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'type' = $1))",
            [entity_type.as_str().to_string()],
        ));
    }

    // ID filter
    if !query.ids.is_empty() {
        let ids: Vec<&str> = query.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col(Alias::new("id")).is_in(ids));
    }

    // ID pattern filter: match against information[].entities[].idPattern
    if let Some(id_pattern) = &query.id_pattern {
        condition = condition.add(SeaExpr::cust_with_values(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'idPattern' = $1))",
            [id_pattern.as_str().to_string()],
        ));
    }

    // Attrs filter: match against information[].propertyNames / relationshipNames
    if !query.attrs.is_empty() {
        let attrs = &query.attrs;
        let placeholders: Vec<String> = (1..=attrs.len()).map(|i| format!("${}", i)).collect();
        let any_list = placeholders.join(", ");
        let sql = format!(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info WHERE \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(info->'propertyNames', '[]'::jsonb)) AS pn WHERE pn IN ({any_list})) OR \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(info->'relationshipNames', '[]'::jsonb)) AS rn WHERE rn IN ({any_list})))"
        );
        let names: Vec<String> = attrs.iter().map(|iri| iri.as_str().to_string()).collect();
        condition = condition.add(SeaExpr::cust_with_values(sql, names));
    }

    // Q expression filter against additional_properties JSONB column
    if let Some(expr) = &query.q_expr {
        condition = condition.add(csr_expr_to_condition(expr)?);
    }

    // Geo filter: a registration that declares no location still matches
    // (clause 5.10.2).
    if let Some(geo) = &query.geo {
        condition = condition.add(geo_sql::condition(geo, GeometrySource::Column { name: "location" }, NullGeometryPolicy::Match)?);
    }

    // Temporal filter: CSRs without the relevant interval still match (per spec)
    if let Some(temporal) = &query.temporal {
        let timeprop = temporal.timeproperty.unwrap_or(TimeProp::Observed);
        let interval_col = match timeprop {
            TimeProp::Created | TimeProp::Modified => "management_interval",
            TimeProp::Observed | TimeProp::Deleted => "observation_interval",
        };

        match temporal.timerel {
            TimeRel::Before => {
                condition = condition.add(SeaExpr::cust_with_values(
                    format!(
                        "(\"{col}\" IS NULL OR (\"{col}\"->>'startAt')::timestamptz < $1::timestamptz)",
                        col = interval_col
                    ),
                    [temporal.time_at.to_rfc3339()],
                ));
            }
            TimeRel::After => {
                condition = condition.add(SeaExpr::cust_with_values(
                    format!(
                        "(\"{col}\" IS NULL OR (\"{col}\"->>'endAt')::timestamptz > $1::timestamptz)",
                        col = interval_col
                    ),
                    [temporal.time_at.to_rfc3339()],
                ));
            }
            TimeRel::Between => {
                let end_time = temporal.end_time_at.as_ref().ok_or(QueryError::ParamEndTimeAtRequired)?;
                condition = condition.add(SeaExpr::cust_with_values(
                    format!(
                        "(\"{col}\" IS NULL OR ((\"{col}\"->>'startAt')::timestamptz <= $2::timestamptz AND (\"{col}\"->>'endAt')::timestamptz >= $1::timestamptz))",
                        col = interval_col
                    ),
                    [temporal.time_at.to_rfc3339(), end_time.to_rfc3339()],
                ));
            }
        }
    }

    Ok(condition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrTemporalFilter, filter::parser, geo::query::GeoQuery, pattern::Pattern};
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;

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
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("context_source_registrations"));
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
    }

    #[test]
    fn build_with_entity_type() {
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ent->>'type' ="));
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = ids(&["urn:a", "urn:b"]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("\"id\" IN"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(Pattern::parse("urn:ngsi-ld:.*").unwrap());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ent->>'idPattern' ="));
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = vec![iri("https://example.com/speed"), iri("https://example.com/temp")];
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("propertyNames"));
        assert!(built.sql.contains("relationshipNames"));
    }

    #[test]
    fn build_with_geo_within() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty_iri: iri("https://uri.etsi.org/ngsi-ld/location"),
        });
        let built = build_csr_query(&q).unwrap();
        // Clause 5.10.2: a registration with no location still matches.
        assert!(built.sql.contains("\"location\" IS NULL"), "{}", built.sql);
        assert!(built.sql.contains("ST_Within"), "{}", built.sql);
    }

    #[test]
    fn build_with_temporal_before() {
        let mut q = empty_query();
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .timerel(TimeRel::Before)
                .time_at(dt("2024-01-01T00:00:00Z"))
                .end_time_at(None)
                .timeproperty(None)
                .build(),
        );
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("observation_interval\" IS NULL"));
        assert!(built.sql.contains("startAt"));
    }

    #[test]
    fn build_with_temporal_after() {
        let mut q = empty_query();
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .timerel(TimeRel::After)
                .time_at(dt("2024-01-01T00:00:00Z"))
                .end_time_at(None)
                .timeproperty(Some(TimeProp::Created))
                .build(),
        );
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("management_interval\" IS NULL"));
        assert!(built.sql.contains("endAt"));
    }

    #[test]
    fn build_with_temporal_between() {
        let mut q = empty_query();
        q.temporal = Some(
            CompiledCsrTemporalFilter::builder()
                .timerel(TimeRel::Between)
                .time_at(dt("2024-01-01T00:00:00Z"))
                .end_time_at(Some(dt("2024-02-01T00:00:00Z")))
                .timeproperty(None)
                .build(),
        );
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("startAt"));
        assert!(built.sql.contains("endAt"));
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(parser::parse("temperature").unwrap());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("\"additional_properties\" ?"));
    }

    #[test]
    fn build_count_sql_always_present() {
        let q = empty_query();
        let built = build_csr_query(&q).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().contains("COUNT(*)"));
    }

    #[test]
    fn build_with_pagination() {
        let mut q = empty_query();
        q.pagination.limit = 50;
        q.pagination.offset = 100;
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
    }

    #[test]
    fn build_with_multiple_filters() {
        let mut q = empty_query();
        q.entity_type = Some(iri("https://example.com/Vehicle"));
        q.ids = ids(&["urn:a"]);
        q.attrs = vec![iri("https://example.com/speed")];
        let built = build_csr_query(&q).unwrap();
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
            geoproperty_iri: iri("https://uri.etsi.org/ngsi-ld/location"),
        });
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ST_Within"), "{}", built.sql);
        assert!(built.sql.contains("ST_GeomFromEWKB"), "{}", built.sql);
    }
}
