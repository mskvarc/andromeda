//! SQL generation for Context Source Registration queries using sea-query.

use crate::{
    core::error::AndromedaError,
    domain::temporal::{TimeProp, TimeRel},
    query::{
        compiled::{CompiledCsrQuery, GeoRel},
        q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
        sql::{BindValue, BuiltQuery},
    },
};
use sea_query::{Alias, Condition, Expr as SeaExpr, Order, PostgresQueryBuilder, Query};
use std::borrow::Cow;

/// Builds a parameterized SQL query to discover Context Source Registrations.
///
/// Generates filter conditions for entity type, IDs, ID patterns, attributes,
/// `q` expressions, geo location, and temporal intervals. CSR-specific semantics
/// apply: registrations without a location or temporal interval still match
/// geo/temporal filters (per NGSI-LD 5.10.2).
pub fn build_csr_query(query: &CompiledCsrQuery) -> Result<BuiltQuery<'static>, AndromedaError> {
    let condition = build_csr_conditions(query)?;

    // Build column list with geometry conversions
    let columns = vec![
        "\"id\"",
        "\"type\"",
        "\"registration_name\"",
        "\"description\"",
        "\"information\"",
        "\"tenant\"",
        "\"observation_interval\"",
        "\"management_interval\"",
        "ST_AsGeoJSON(\"location\")::JSONB AS \"location\"",
        "ST_AsGeoJSON(\"observation_space\")::JSONB AS \"observation_space\"",
        "ST_AsGeoJSON(\"operation_space\")::JSONB AS \"operation_space\"",
        "\"expires_at\"",
        "\"endpoint\"",
        "\"accept\"",
        "\"mode\"",
        "\"operations\"",
        "\"scope\"",
        "\"refresh_rate\"",
        "\"management\"",
        "\"context_source_info\"",
        "\"jsonld_context\"",
        "\"context_source_alias\"",
        "\"status\"",
        "\"times_sent\"",
        "\"times_failed\"",
        "\"last_success\"",
        "\"last_failure\"",
        "\"created_at\"",
        "\"modified_at\"",
        "\"additional_properties\"",
    ];

    let mut select = Query::select();
    select
        .expr(SeaExpr::cust(columns.join(", ")))
        .from(Alias::new("context_source_registrations"))
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
fn build_csr_conditions(query: &CompiledCsrQuery) -> Result<Condition, AndromedaError> {
    let mut condition = Condition::all();

    // Entity type filter: match against information[].entities[].type
    if let Some(ref entity_type) = query.entity_type {
        condition = condition.add(SeaExpr::cust_with_values(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'type' = $1))",
            [entity_type.clone()],
        ));
    }

    // ID filter
    if let Some(ref ids) = query.ids {
        condition = condition.add(SeaExpr::col(Alias::new("id")).is_in(ids.clone()));
    }

    // ID pattern filter: match against information[].entities[].idPattern
    if let Some(ref id_pattern) = query.id_pattern {
        condition = condition.add(SeaExpr::cust_with_values(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'idPattern' = $1))",
            [id_pattern.as_str().to_string()],
        ));
    }

    // Attrs filter: match against information[].propertyNames / relationshipNames
    if let Some(ref attrs) = query.attrs
        && !attrs.is_empty()
    {
        let placeholders: Vec<String> = (1..=attrs.len()).map(|i| format!("${}", i)).collect();
        let any_list = placeholders.join(", ");
        let sql = format!(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(\"information\") AS info WHERE \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(info->'propertyNames', '[]'::jsonb)) AS pn WHERE pn IN ({any_list})) OR \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(info->'relationshipNames', '[]'::jsonb)) AS rn WHERE rn IN ({any_list})))"
        );
        let values: Vec<String> = attrs.to_vec();
        condition = condition.add(SeaExpr::cust_with_values(sql, values));
    }

    // Q expression filter against additional_properties JSONB column
    if let Some(ref expr) = query.q_expr {
        let q_cond = csr_expr_to_condition(expr)?;
        condition = condition.add(q_cond);
    }

    // Geo filter: CSRs without a location still match (per NGSI-LD spec)
    if let Some(ref geo) = query.geo {
        let geojson = build_geojson(&geo.geometry, &geo.coordinates)?;
        // Escape single quotes for safe SQL embedding (geojson is valid JSON, so no unescaped quotes expected, but be defensive)
        let geojson_escaped = geojson.replace('\'', "''");
        // Inline the geometry directly to avoid bind parameter issues with sea-query expression composition
        let ref_geom_sql = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))", geojson_escaped);

        let spatial_sql = match &geo.georel {
            GeoRel::Within => format!("ST_Within(\"location\", {})", ref_geom_sql),
            GeoRel::Contains => format!("ST_Contains(\"location\", {})", ref_geom_sql),
            GeoRel::Intersects => format!("ST_Intersects(\"location\", {})", ref_geom_sql),
            GeoRel::Disjoint => format!("ST_Disjoint(\"location\", {})", ref_geom_sql),
            GeoRel::Equals => format!("ST_Equals(\"location\", {})", ref_geom_sql),
            GeoRel::Overlaps => format!("ST_Overlaps(\"location\", {})", ref_geom_sql),
            GeoRel::Near { max_distance, min_distance } => {
                let ref_geom_geo_sql = format!("{}::geography", ref_geom_sql);

                let mut parts = Vec::new();
                if let Some(max) = max_distance {
                    parts.push(format!("ST_DWithin(\"location\"::geography, {}, {})", ref_geom_geo_sql, max));
                }
                if let Some(min) = min_distance {
                    parts.push(format!("ST_Distance(\"location\"::geography, {}) >= {}", ref_geom_geo_sql, min));
                }
                if parts.is_empty() {
                    return Err(AndromedaError::query_validation("near georel requires maxDistance or minDistance"));
                }
                parts.join(" AND ")
            }
        };

        condition = condition.add(SeaExpr::cust(format!("(\"location\" IS NULL OR {})", spatial_sql)));
    }

    // Temporal filter: CSRs without the relevant interval still match (per spec)
    if let Some(ref temporal) = query.temporal {
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
                let end_time = temporal
                    .end_time_at
                    .as_ref()
                    .ok_or_else(|| AndromedaError::query_validation("endTimeAt is required when timerel=between"))?;
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

/// Build a GeoJSON string from NGSI-LD geometry type and coordinates.
fn build_geojson(geometry: &str, coordinates: &serde_json::Value) -> Result<String, AndromedaError> {
    let coords_value = match geometry {
        "Polygon" => {
            if let Some(arr) = coordinates.as_array() {
                if let Some(first) = arr.first()
                    && let Some(inner) = first.as_array()
                    && !inner.is_empty()
                    && !inner[0].is_array()
                {
                    let mut ring = arr.clone();
                    if ring.first() != ring.last() {
                        ring.push(ring[0].clone());
                    }
                    Cow::Owned(serde_json::Value::Array(vec![serde_json::Value::Array(ring)]))
                } else {
                    Cow::Borrowed(coordinates)
                }
            } else {
                Cow::Borrowed(coordinates)
            }
        }
        _ => Cow::Borrowed(coordinates),
    };

    let geojson = serde_json::json!({
        "type": geometry,
        "coordinates": *coords_value
    });
    Ok(geojson.to_string())
}

// --- Q expression to SQL for CSR additional_properties ---

fn csr_expr_to_condition(expr: &Expr) -> Result<Condition, AndromedaError> {
    match expr {
        Expr::And(terms) => {
            let mut cond = Condition::all();
            for term in terms {
                cond = cond.add(csr_expr_to_condition(term)?);
            }
            Ok(cond)
        }
        Expr::Or(terms) => {
            let mut cond = Condition::any();
            for term in terms {
                cond = cond.add(csr_expr_to_condition(term)?);
            }
            Ok(cond)
        }
        Expr::Pred(pred) => csr_pred_to_condition(pred),
    }
}

fn csr_pred_to_condition(pred: &Predicate) -> Result<Condition, AndromedaError> {
    match pred {
        Predicate::Exists(path) => Ok(Condition::all().add(SeaExpr::cust_with_values("\"additional_properties\" ? $1", [path.attr.clone()]))),
        Predicate::Compare { path, op, rhs } => csr_compare_to_condition(path, *op, rhs),
        Predicate::MatchPattern { path, pattern } => {
            let extract = csr_value_extract(path);
            Ok(Condition::all().add(SeaExpr::cust_with_values(
                format!("({} IS NOT NULL AND ({})::text ~ $1)", extract, extract),
                [pattern.clone()],
            )))
        }
        Predicate::NotMatchPattern { path, pattern } => {
            let extract = csr_value_extract(path);
            Ok(Condition::all().add(SeaExpr::cust_with_values(
                format!("({} IS NULL OR NOT ({})::text ~ $1)", extract, extract),
                [pattern.clone()],
            )))
        }
    }
}

/// Build a SQL expression that extracts the value of a CSR property from additional_properties.
fn csr_value_extract(path: &AttrPath) -> String {
    let mut json_path = format!("\"additional_properties\"->'{}'", escape_sql_string(&path.attr));

    // Unwrap Property wrapper: use ->'value' if it's a Property
    json_path = format!("CASE WHEN ({jp}->>'type') = 'Property' THEN {jp}->'value' ELSE {jp} END", jp = json_path);

    for sub in &path.sub_attrs {
        let prev = json_path.clone();
        json_path = format!("({prev})->'{}'", escape_sql_string(sub));
        json_path = format!("CASE WHEN ({jp}->>'type') = 'Property' THEN {jp}->'value' ELSE {jp} END", jp = json_path);
    }

    json_path
}

fn csr_compare_to_condition(path: &AttrPath, op: CmpOp, rhs: &CompareValue) -> Result<Condition, AndromedaError> {
    let extract = csr_value_extract(path);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let op_str = cmp_op_sql(op);
            let (cast, value) = scalar_to_value(scalar);
            let sql = format!(
                "(({extract}) IS NOT NULL AND (({extract}) #>> '{{}}'){cast} {op_str} $1{cast})",
                extract = extract,
                cast = cast,
                op_str = op_str,
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [value])))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_val) = scalar_to_value(min);
            let (_, max_val) = scalar_to_value(max);
            let sql = match op {
                CmpOp::Equal => format!(
                    "(({extract}) IS NOT NULL AND (({extract}) #>> '{{}}'){cast} >= $1{cast} AND (({extract}) #>> '{{}}'){cast} <= $2{cast})",
                    extract = extract,
                    cast = cast,
                ),
                CmpOp::Unequal => format!(
                    "(({extract}) IS NULL OR (({extract}) #>> '{{}}'){cast} < $1{cast} OR (({extract}) #>> '{{}}'){cast} > $2{cast})",
                    extract = extract,
                    cast = cast,
                ),
                _ => return Err(AndromedaError::query_validation("Range values only supported with == and != operators")),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [min_val, max_val])))
        }
        CompareValue::ValueList(values) => {
            let mut parts = Vec::with_capacity(values.len());
            let mut bind_values = Vec::with_capacity(values.len());
            for (i, v) in values.iter().enumerate() {
                let (cast, val) = scalar_to_value(v);
                parts.push(format!(
                    "(({extract}) #>> '{{}}'){cast} = ${idx}{cast}",
                    extract = extract,
                    cast = cast,
                    idx = i + 1
                ));
                bind_values.push(val);
            }
            let joined = parts.join(" OR ");
            let sql = match op {
                CmpOp::Equal => format!("(({extract}) IS NOT NULL AND ({}))", joined, extract = extract),
                CmpOp::Unequal => format!("(({extract}) IS NULL OR NOT ({}))", joined, extract = extract),
                _ => return Err(AndromedaError::query_validation("Value lists only supported with == and != operators")),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, bind_values)))
        }
    }
}

fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

const fn cmp_op_sql(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

fn scalar_to_value(scalar: &Scalar) -> (&'static str, String) {
    match scalar {
        Scalar::Number(n) => ("::numeric", n.to_string()),
        Scalar::Bool(b) => ("::boolean", b.to_string()),
        Scalar::Str(s) => ("", s.clone()),
        Scalar::DateTime(s) | Scalar::Date(s) | Scalar::Time(s) => ("", s.clone()),
    }
}

/// Converts sea-query Values to BindValue vector for repository compatibility.
fn sea_values_to_bind_values(values: sea_query::Values) -> Result<Vec<BindValue<'static>>, AndromedaError> {
    values
        .0
        .into_iter()
        .map(|v| match v {
            sea_query::Value::String(Some(s)) => Ok(BindValue::String(Cow::Owned(s.to_string()))),
            sea_query::Value::Int(Some(i)) => Ok(BindValue::I64(i64::from(i))),
            sea_query::Value::BigInt(Some(i)) => Ok(BindValue::I64(i)),
            sea_query::Value::BigUnsigned(Some(u)) => Ok(BindValue::I64(u as i64)),
            sea_query::Value::Unsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            sea_query::Value::TinyInt(Some(i)) => Ok(BindValue::I64(i64::from(i))),
            sea_query::Value::TinyUnsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            sea_query::Value::SmallInt(Some(i)) => Ok(BindValue::I64(i64::from(i))),
            sea_query::Value::SmallUnsigned(Some(u)) => Ok(BindValue::I64(i64::from(u))),
            sea_query::Value::Bool(Some(b)) => Ok(BindValue::String(Cow::Owned(b.to_string()))),
            sea_query::Value::Double(Some(f)) => Ok(BindValue::String(Cow::Owned(f.to_string()))),
            sea_query::Value::Float(Some(f)) => Ok(BindValue::String(Cow::Owned(f.to_string()))),
            other => Err(AndromedaError::internal(format!(
                "unexpected sea_query::Value variant in bind conversion: {other:?}"
            ))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::{
        domain::temporal::{TimeProp, TimeRel},
        query::{
            compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter, GeoQuery, IdPattern},
            q::parser,
        },
    };
    use chrono::{DateTime, Utc};

    fn dt(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn empty_query() -> CompiledCsrQuery {
        CompiledCsrQuery {
            ids: None,
            entity_type: None,
            id_pattern: None,
            attrs: None,
            q_expr: None,
            geo: None,
            temporal: None,
            pagination: crate::query::compiled::Pagination::DEFAULT,
        }
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
        q.entity_type = Some("https://example.com/Vehicle".to_string());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ent->>'type' ="));
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = Some(vec!["urn:a".to_string(), "urn:b".to_string()]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("\"id\" IN"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(IdPattern::new("urn:ngsi-ld:.*").unwrap());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ent->>'idPattern' ="));
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = Some(vec!["https://example.com/speed".to_string(), "https://example.com/temp".to_string()]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("propertyNames"));
        assert!(built.sql.contains("relationshipNames"));
    }

    #[test]
    fn build_with_geo_within() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([1.0, 2.0]),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("\"location\" IS NULL OR ST_Within"));
    }

    #[test]
    fn build_with_temporal_before() {
        let mut q = empty_query();
        q.temporal = Some(CompiledCsrTemporalFilter {
            timerel: TimeRel::Before,
            time_at: dt("2024-01-01T00:00:00Z"),
            end_time_at: None,
            timeproperty: None,
        });
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("observation_interval\" IS NULL"));
        assert!(built.sql.contains("startAt"));
    }

    #[test]
    fn build_with_temporal_after() {
        let mut q = empty_query();
        q.temporal = Some(CompiledCsrTemporalFilter {
            timerel: TimeRel::After,
            time_at: dt("2024-01-01T00:00:00Z"),
            end_time_at: None,
            timeproperty: Some(TimeProp::Created),
        });
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("management_interval\" IS NULL"));
        assert!(built.sql.contains("endAt"));
    }

    #[test]
    fn build_with_temporal_between() {
        let mut q = empty_query();
        q.temporal = Some(CompiledCsrTemporalFilter {
            timerel: TimeRel::Between,
            time_at: dt("2024-01-01T00:00:00Z"),
            end_time_at: Some(dt("2024-02-01T00:00:00Z")),
            timeproperty: None,
        });
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
        q.entity_type = Some("https://example.com/Vehicle".to_string());
        q.ids = Some(vec!["urn:a".to_string()]);
        q.attrs = Some(vec!["https://example.com/speed".to_string()]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains(" AND "));
    }
}
