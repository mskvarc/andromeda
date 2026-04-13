//! SQL generation for Context Source Registration queries.

use crate::{
    core::error::AndromedaError,
    domain::temporal::{TimeProp, TimeRel},
    query::{
        compiled::{CompiledCsrQuery, GeoRel},
        q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate},
        sql::{BindValue, BuiltQuery, cmp_op_sql, scalar_to_sql_bind},
    },
};
use std::borrow::Cow;

/// Builds a parameterized SQL query to discover Context Source Registrations.
///
/// Generates filter conditions for entity type, IDs, ID patterns, attributes,
/// `q` expressions, geo location, and temporal intervals. CSR-specific semantics
/// apply: registrations without a location or temporal interval still match
/// geo/temporal filters (per NGSI-LD 5.10.2).
pub fn build_csr_query<'a>(query: &'a CompiledCsrQuery) -> Result<BuiltQuery<'a>, AndromedaError> {
    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<BindValue<'a>> = Vec::new();
    let mut param_idx = 1usize;

    // Entity type filter: match against information[].entities[].type
    if let Some(ref entity_type) = query.entity_type {
        bind_condition!(
            conditions,
            binds,
            &mut param_idx,
            "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'type' = ${}))",
            BindValue::String(Cow::Borrowed(entity_type.as_str()))
        );
    }

    // ID filter
    if let Some(ref ids) = query.ids {
        bind_condition!(
            conditions,
            binds,
            &mut param_idx,
            "id = ANY(${})",
            BindValue::StringVec(Cow::Borrowed(ids.as_slice()))
        );
    }

    // ID pattern filter: match against information[].entities[].idPattern
    if let Some(ref id_pattern) = query.id_pattern {
        bind_condition!(
            conditions,
            binds,
            &mut param_idx,
            "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent WHERE ent->>'idPattern' = ${}))",
            BindValue::String(Cow::Borrowed(id_pattern.as_str()))
        );
    }

    // Attrs filter: match against information[].propertyNames / relationshipNames
    if let Some(ref attrs) = query.attrs
        && !attrs.is_empty()
    {
        let attr_placeholders: Vec<String> = attrs.iter().enumerate().map(|(i, _)| format!("${}", param_idx + i)).collect();
        let any_list = attr_placeholders.join(", ");
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info WHERE \
                 EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(info->'propertyNames', '[]'::jsonb)) AS pn WHERE pn IN ({any_list})) OR \
                 EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(info->'relationshipNames', '[]'::jsonb)) AS rn WHERE rn IN ({any_list})))"
        ));
        for attr in attrs {
            binds.push(BindValue::String(Cow::Borrowed(attr.as_str())));
        }
        param_idx += attrs.len();
    }

    // Q expression filter against additional_properties JSONB column
    if let Some(ref expr) = query.q_expr {
        let q_sql = csr_expr_to_sql(expr, &mut binds, &mut param_idx)?;
        conditions.push(q_sql);
    }

    // Geo filter: CSRs without a location still match (per NGSI-LD spec)
    if let Some(ref geo) = query.geo {
        let geojson = build_geojson(&geo.geometry, &geo.coordinates)?;
        let ref_geom = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(${}), 4326))", param_idx);
        binds.push(BindValue::String(Cow::Owned(geojson)));
        let spatial_cond = match &geo.georel {
            GeoRel::Within => format!("ST_Within(location, {})", ref_geom),
            GeoRel::Contains => format!("ST_Contains(location, {})", ref_geom),
            GeoRel::Intersects => format!("ST_Intersects(location, {})", ref_geom),
            GeoRel::Disjoint => format!("ST_Disjoint(location, {})", ref_geom),
            GeoRel::Equals => format!("ST_Equals(location, {})", ref_geom),
            GeoRel::Overlaps => format!("ST_Overlaps(location, {})", ref_geom),
            GeoRel::Near { max_distance, min_distance } => {
                let mut parts = Vec::new();
                if let Some(max) = max_distance {
                    parts.push(format!("ST_DWithin(location::geography, {}::geography, {})", ref_geom, max));
                }
                if let Some(min) = min_distance {
                    parts.push(format!("ST_Distance(location::geography, {}::geography) >= {}", ref_geom, min));
                }
                if parts.is_empty() {
                    return Err(AndromedaError::query_validation("near georel requires maxDistance or minDistance"));
                }
                parts.join(" AND ")
            }
        };
        conditions.push(format!("(location IS NULL OR {})", spatial_cond));
        param_idx += 1;
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
                conditions.push(format!(
                    "({col} IS NULL OR ({col}->>'startAt')::timestamptz < ${}::timestamptz)",
                    param_idx,
                    col = interval_col,
                ));
                binds.push(BindValue::String(Cow::Owned(temporal.time_at.to_rfc3339())));
                param_idx += 1;
            }
            TimeRel::After => {
                conditions.push(format!(
                    "({col} IS NULL OR ({col}->>'endAt')::timestamptz > ${}::timestamptz)",
                    param_idx,
                    col = interval_col,
                ));
                binds.push(BindValue::String(Cow::Owned(temporal.time_at.to_rfc3339())));
                param_idx += 1;
            }
            TimeRel::Between => {
                let end_time = temporal
                    .end_time_at
                    .as_ref()
                    .ok_or_else(|| AndromedaError::query_validation("endTimeAt is required when timerel=between"))?;
                conditions.push(format!(
                    "({col} IS NULL OR (({col}->>'startAt')::timestamptz <= ${}::timestamptz AND ({col}->>'endAt')::timestamptz >= ${}::timestamptz))",
                    param_idx + 1,
                    param_idx,
                    col = interval_col,
                ));
                binds.push(BindValue::String(Cow::Owned(temporal.time_at.to_rfc3339())));
                binds.push(BindValue::String(Cow::Owned(end_time.to_rfc3339())));
                param_idx += 2;
            }
        }
    }

    let _ = param_idx;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let columns = "\
        id, type, registration_name, description, information, \
        tenant, observation_interval, management_interval, \
        ST_AsGeoJSON(location)::JSONB AS location, \
        ST_AsGeoJSON(observation_space)::JSONB AS observation_space, \
        ST_AsGeoJSON(operation_space)::JSONB AS operation_space, \
        expires_at, endpoint, accept, mode, \
        operations, scope, refresh_rate, \
        management, context_source_info, jsonld_context, context_source_alias, \
        status, times_sent, times_failed, \
        last_success, last_failure, created_at, modified_at, \
        additional_properties";

    let sql = format!(
        "SELECT {} FROM context_source_registrations{} ORDER BY created_at ASC LIMIT {} OFFSET {}",
        columns, where_clause, query.limit, query.offset
    );

    let count_sql = Some(format!("SELECT COUNT(*) FROM context_source_registrations{}", where_clause));

    Ok(BuiltQuery {
        sql,
        count_sql,
        bind_values: binds,
    })
}

/// Build a GeoJSON string from NGSI-LD geometry type and coordinates.
/// Handles Polygon coordinate wrapping (flat point list -> ring array + close).
fn build_geojson(geometry: &str, coordinates: &serde_json::Value) -> Result<String, AndromedaError> {
    let coords_value = match geometry {
        "Polygon" => {
            if let Some(arr) = coordinates.as_array() {
                if let Some(first) = arr.first()
                    && let Some(inner) = first.as_array()
                    && !inner.is_empty()
                    && !inner[0].is_array()
                {
                    // Clone needed: wrapping flat point list into a ring array for GeoJSON Polygon format
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
// CSR properties are stored in compact form in additional_properties JSONB column.
// Properties may be wrapped as {"type":"Property","value":...}.

fn csr_expr_to_sql(expr: &Expr, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize) -> Result<String, AndromedaError> {
    match expr {
        Expr::And(terms) => {
            let parts: Result<Vec<String>, _> = terms.iter().map(|t| csr_expr_to_sql(t, binds, param_idx)).collect();
            Ok(format!("({})", parts?.join(" AND ")))
        }
        Expr::Or(terms) => {
            let parts: Result<Vec<String>, _> = terms.iter().map(|t| csr_expr_to_sql(t, binds, param_idx)).collect();
            Ok(format!("({})", parts?.join(" OR ")))
        }
        Expr::Pred(pred) => csr_pred_to_sql(pred, binds, param_idx),
    }
}

fn csr_pred_to_sql(pred: &Predicate, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize) -> Result<String, AndromedaError> {
    match pred {
        Predicate::Exists(path) => {
            let sql = format!("additional_properties ? ${}", *param_idx);
            binds.push(BindValue::String(Cow::Owned(path.attr.clone())));
            *param_idx += 1;
            Ok(sql)
        }
        Predicate::Compare { path, op, rhs } => csr_compare_to_sql(path, *op, rhs, binds, param_idx),
        Predicate::MatchPattern { path, pattern } => {
            let extract = csr_value_extract(path, binds, param_idx);
            let sql = format!("({} IS NOT NULL AND ({})::text ~ ${})", extract, extract, *param_idx);
            binds.push(BindValue::String(Cow::Owned(pattern.clone())));
            *param_idx += 1;
            Ok(sql)
        }
        Predicate::NotMatchPattern { path, pattern } => {
            let extract = csr_value_extract(path, binds, param_idx);
            let sql = format!("({} IS NULL OR NOT ({})::text ~ ${})", extract, extract, *param_idx);
            binds.push(BindValue::String(Cow::Owned(pattern.clone())));
            *param_idx += 1;
            Ok(sql)
        }
    }
}

/// Build a SQL expression that extracts the value of a CSR property from additional_properties.
/// Handles Property wrapper: if the property has {"type":"Property","value":X}, extract X.
/// For sub-attributes, navigates through nested objects.
fn csr_value_extract(path: &AttrPath, _binds: &mut Vec<BindValue<'_>>, _param_idx: &mut usize) -> String {
    let mut json_path = format!("additional_properties->'{}'", escape_sql_string(&path.attr));

    // Unwrap Property wrapper: use ->'value' if it's a Property
    json_path = format!("CASE WHEN ({jp}->>'type') = 'Property' THEN {jp}->'value' ELSE {jp} END", jp = json_path);

    for sub in &path.sub_attrs {
        let prev = json_path.clone();
        json_path = format!("({prev})->'{}'", escape_sql_string(sub));
        // Unwrap nested Property wrapper
        json_path = format!("CASE WHEN ({jp}->>'type') = 'Property' THEN {jp}->'value' ELSE {jp} END", jp = json_path);
    }

    json_path
}

fn csr_compare_to_sql(path: &AttrPath, op: CmpOp, rhs: &CompareValue, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize) -> Result<String, AndromedaError> {
    let extract = csr_value_extract(path, binds, param_idx);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let op_str = cmp_op_sql(op);
            let (cast, bind_val) = scalar_to_sql_bind(scalar);
            let sql = format!(
                "(({extract}) IS NOT NULL AND (({extract}) #>> '{{}}'){cast} {op_str} ${idx}{cast})",
                extract = extract,
                cast = cast,
                op_str = op_str,
                idx = *param_idx,
            );
            binds.push(bind_val);
            *param_idx += 1;
            Ok(sql)
        }
        CompareValue::Range { min, max } => {
            let (cast, bind_min) = scalar_to_sql_bind(min);
            let (_, bind_max) = scalar_to_sql_bind(max);
            let sql = match op {
                CmpOp::Equal => {
                    let s = format!(
                        "(({extract}) IS NOT NULL AND (({extract}) #>> '{{}}'){cast} >= ${p1}{cast} AND (({extract}) #>> '{{}}'){cast} <= ${p2}{cast})",
                        extract = extract,
                        cast = cast,
                        p1 = *param_idx,
                        p2 = *param_idx + 1,
                    );
                    binds.push(bind_min);
                    binds.push(bind_max);
                    *param_idx += 2;
                    s
                }
                CmpOp::Unequal => {
                    let s = format!(
                        "(({extract}) IS NULL OR (({extract}) #>> '{{}}'){cast} < ${p1}{cast} OR (({extract}) #>> '{{}}'){cast} > ${p2}{cast})",
                        extract = extract,
                        cast = cast,
                        p1 = *param_idx,
                        p2 = *param_idx + 1,
                    );
                    binds.push(bind_min);
                    binds.push(bind_max);
                    *param_idx += 2;
                    s
                }
                _ => return Err(AndromedaError::query_validation("Range values only supported with == and != operators")),
            };
            Ok(sql)
        }
        CompareValue::ValueList(values) => {
            let parts: Vec<String> = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let (cast, bind_val) = scalar_to_sql_bind(v);
                    let idx = *param_idx + i;
                    binds.push(bind_val);
                    format!("(({extract}) #>> '{{}}'){cast} = ${idx}{cast}", extract = extract, cast = cast, idx = idx)
                })
                .collect();
            *param_idx += values.len();
            match op {
                CmpOp::Equal => Ok(format!("(({extract}) IS NOT NULL AND ({}))", parts.join(" OR "), extract = extract)),
                CmpOp::Unequal => Ok(format!("(({extract}) IS NULL OR NOT ({}))", parts.join(" OR "), extract = extract)),
                _ => Err(AndromedaError::query_validation("Value lists only supported with == and != operators")),
            }
        }
    }
}

fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
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
            limit: 20,
            offset: 0,
        }
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("FROM context_source_registrations"));
        assert!(built.sql.contains("LIMIT 20 OFFSET 0"));
        assert!(built.bind_values.is_empty());
    }

    #[test]
    fn build_with_entity_type() {
        let mut q = empty_query();
        q.entity_type = Some("https://example.com/Vehicle".to_string());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ent->>'type' = $1"));
        pretty_assertions::assert_eq!(built.bind_values.len(), 1);
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = Some(vec!["urn:a".to_string(), "urn:b".to_string()]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("id = ANY($1)"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(IdPattern::new("urn:ngsi-ld:.*").unwrap());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("ent->>'idPattern' = $1"));
    }

    #[test]
    fn build_with_attrs() {
        let mut q = empty_query();
        q.attrs = Some(vec!["https://example.com/speed".to_string(), "https://example.com/temp".to_string()]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("propertyNames"));
        assert!(built.sql.contains("relationshipNames"));
        pretty_assertions::assert_eq!(built.bind_values.len(), 2);
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
        assert!(built.sql.contains("location IS NULL OR ST_Within"));
        pretty_assertions::assert_eq!(built.bind_values.len(), 1);
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
        assert!(built.sql.contains("observation_interval IS NULL"));
        assert!(built.sql.contains("startAt"));
        pretty_assertions::assert_eq!(built.bind_values.len(), 1);
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
        assert!(built.sql.contains("management_interval IS NULL"));
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
        pretty_assertions::assert_eq!(built.bind_values.len(), 2);
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(parser::parse("temperature").unwrap());
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("additional_properties ? $1"));
    }

    #[test]
    fn build_count_sql_always_present() {
        let q = empty_query();
        let built = build_csr_query(&q).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().starts_with("SELECT COUNT(*)"));
    }

    #[test]
    fn build_with_pagination() {
        let mut q = empty_query();
        q.limit = 50;
        q.offset = 100;
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains("LIMIT 50 OFFSET 100"));
    }

    #[test]
    fn build_with_multiple_filters() {
        let mut q = empty_query();
        q.entity_type = Some("https://example.com/Vehicle".to_string());
        q.ids = Some(vec!["urn:a".to_string()]);
        q.attrs = Some(vec!["https://example.com/speed".to_string()]);
        let built = build_csr_query(&q).unwrap();
        assert!(built.sql.contains(" AND "));
        pretty_assertions::assert_eq!(built.bind_values.len(), 3);
    }
}
