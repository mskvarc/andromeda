use crate::{
    error::AndromedaError,
    query::{
        compiled::{CompiledEntityQuery, GeoQuery, GeoRel},
        q::{CmpOp, CompareValue, Expr, Predicate, Scalar},
    },
};
use serde_json::Value;

#[derive(Debug)]
pub struct BuiltQuery {
    pub sql: String,
    pub count_sql: Option<String>,
    pub bind_values: Vec<BindValue>,
}

#[derive(Debug, Clone)]
pub enum BindValue {
    String(String),
    StringVec(Vec<String>),
    #[allow(dead_code)]
    I64(i64),
}

pub fn build_query(query: &CompiledEntityQuery) -> Result<BuiltQuery, AndromedaError> {
    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<BindValue> = Vec::new();
    let mut param_idx = 1usize;

    if let Some(ids) = &query.ids {
        conditions.push(format!("id = ANY(${})", param_idx));
        binds.push(BindValue::StringVec(ids.clone()));
        param_idx += 1;
    }

    if let Some(pattern) = &query.id_pattern {
        conditions.push(format!("id ~ ${}", param_idx));
        binds.push(BindValue::String(pattern.clone()));
        param_idx += 1;
    }

    if let Some(types) = &query.types {
        if types.len() == 1 {
            conditions.push(format!("${} = ANY(types)", param_idx));
            binds.push(BindValue::String(types[0].clone()));
            param_idx += 1;
        } else if query.type_match_all {
            for t in types {
                conditions.push(format!("${} = ANY(types)", param_idx));
                binds.push(BindValue::String(t.clone()));
                param_idx += 1;
            }
        } else {
            conditions.push(format!("types && ${}", param_idx));
            binds.push(BindValue::StringVec(types.clone()));
            param_idx += 1;
        }
    }

    if !query.attrs.is_empty() {
        let attr_conditions: Vec<String> = query
            .attrs
            .iter()
            .map(|attr| {
                let cond = format!("payload->0 ? ${}", param_idx);
                binds.push(BindValue::String(attr.clone()));
                param_idx += 1;
                cond
            })
            .collect();
        conditions.push(format!("({})", attr_conditions.join(" OR ")));
    }

    if let Some(expr) = &query.q_expr {
        let q_sql = expr_to_sql(expr, &mut binds, &mut param_idx, "payload")?;
        conditions.push(q_sql);
    }

    if let Some(geo) = &query.geo {
        let geo_sql = geo_to_sql(geo, &mut binds, &mut param_idx, "payload")?;
        conditions.push(geo_sql);
    }

    if let Some(scope_q) = &query.scope_q {
        let scope_sql = scope_q_to_sql(scope_q, &mut binds, &mut param_idx)?;
        conditions.push(scope_sql);
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let order_clause = if query.order_by.is_empty() {
        String::new()
    } else {
        let order_parts: Vec<String> = query
            .order_by
            .iter()
            .map(|term| {
                let dir = term.direction.sql_dir();
                if term.direction.is_distance() {
                    // Distance-based ordering using ST_Distance with Haversine (geography type)
                    // Requires a geo query to provide the reference geometry
                    if let Some(geo) = &query.geo {
                        let geo_extract = build_geo_extract(&geo.geoproperty_iri, "payload");
                        let ref_geom = format!("ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326)", build_geojson(&geo.geometry, &geo.coordinates)?);
                        Ok(format!("ST_Distance({}::geography, {}::geography) {} NULLS LAST", geo_extract, ref_geom, dir))
                    } else {
                        // Fallback: no geo query, treat as regular ordering
                        let json_path = property_value_jsonpath(&term.attr_iri);
                        Ok(format!("jsonb_path_query_first(payload, '{}') {} NULLS LAST", json_path, dir))
                    }
                } else {
                    let json_path = property_value_jsonpath(&term.attr_iri);
                    let collation_clause = term.collation.as_ref().map(|c| format!(" COLLATE \"{}\"", c)).unwrap_or_default();
                    Ok(format!(
                        "jsonb_path_query_first(payload, '{}'){} {} NULLS LAST",
                        json_path, collation_clause, dir
                    ))
                }
            })
            .collect::<Result<Vec<_>, AndromedaError>>()?;
        format!(" ORDER BY {}, id ASC", order_parts.join(", "))
    };

    let pagination = format!(" LIMIT {} OFFSET {}", query.limit, query.offset);

    let sql = format!("SELECT payload FROM entities{}{}{}", where_clause, order_clause, pagination);

    let count_sql = if query.count {
        Some(format!("SELECT COUNT(*) FROM entities{}", where_clause))
    } else {
        None
    };

    Ok(BuiltQuery {
        sql,
        count_sql,
        bind_values: binds,
    })
}

fn expr_to_sql(expr: &Expr, binds: &mut Vec<BindValue>, param_idx: &mut usize, payload_column: &str) -> Result<String, AndromedaError> {
    match expr {
        Expr::And(terms) => {
            let parts: Result<Vec<String>, _> = terms.iter().map(|t| expr_to_sql(t, binds, param_idx, payload_column)).collect();
            Ok(format!("({})", parts?.join(" AND ")))
        }
        Expr::Or(terms) => {
            let parts: Result<Vec<String>, _> = terms.iter().map(|t| expr_to_sql(t, binds, param_idx, payload_column)).collect();
            Ok(format!("({})", parts?.join(" OR ")))
        }
        Expr::Pred(pred) => pred_to_sql(pred, binds, param_idx, payload_column),
    }
}

fn pred_to_sql(pred: &Predicate, binds: &mut Vec<BindValue>, param_idx: &mut usize, payload_column: &str) -> Result<String, AndromedaError> {
    match pred {
        Predicate::Exists(path) => {
            let attr_iri = &path.attr;
            let sql = format!("{}->0 ? ${}", payload_column, *param_idx);
            binds.push(BindValue::String(attr_iri.clone()));
            *param_idx += 1;
            Ok(sql)
        }
        Predicate::Compare { path, op, rhs } => compare_to_sql(path, *op, rhs, binds, param_idx, payload_column),
        Predicate::MatchPattern { path, pattern } => {
            let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
            let sql = format!(
                "jsonb_path_exists({}, '{}') AND (jsonb_path_query_first({}, '{}')#>>'{{}}')::text ~ ${}",
                payload_column, jsonpath, payload_column, jsonpath, *param_idx
            );
            binds.push(BindValue::String(pattern.clone()));
            *param_idx += 1;
            Ok(sql)
        }
        Predicate::NotMatchPattern { path, pattern } => {
            let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
            let sql = format!(
                "NOT (jsonb_path_exists({}, '{}') AND (jsonb_path_query_first({}, '{}')#>>'{{}}')::text ~ ${})",
                payload_column, jsonpath, payload_column, jsonpath, *param_idx
            );
            binds.push(BindValue::String(pattern.clone()));
            *param_idx += 1;
            Ok(sql)
        }
    }
}

fn compare_to_sql(
    path: &crate::query::q::AttrPath,
    op: CmpOp,
    rhs: &CompareValue,
    binds: &mut Vec<BindValue>,
    param_idx: &mut usize,
    payload_column: &str,
) -> Result<String, AndromedaError> {
    let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
    let extract_expr = format!("jsonb_path_query_first({}, '{}')", payload_column, jsonpath);

    match rhs {
        CompareValue::Scalar(scalar) => {
            let op_str = cmp_op_sql(op);
            let (cast, bind_val) = scalar_to_sql_bind(scalar);
            // Use COALESCE to try hasValue first, then hasObject (for Relationships)
            let has_object_jsonpath =
                value_extraction_jsonpath_with_predicate(&path.attr, &path.sub_attrs, &path.value_path, "https://uri.etsi.org/ngsi-ld/hasObject");
            let extract_object = format!("jsonb_path_query_first({}, '{}')", payload_column, has_object_jsonpath);
            let sql = format!(
                "(COALESCE(({} #>> '{{}}'), ({} #>> '{{}}')){} {} ${}{})",
                extract_expr, extract_object, cast, op_str, *param_idx, cast
            );
            binds.push(bind_val);
            *param_idx += 1;
            Ok(sql)
        }
        CompareValue::Range { min, max } => {
            let (cast_min, bind_min) = scalar_to_sql_bind(min);
            let (_cast_max, bind_max) = scalar_to_sql_bind(max);
            let cast = cast_min;
            let sql = match op {
                CmpOp::Equal => {
                    let s = format!(
                        "(({} #>> '{{}}')){} >= ${}{} AND (({} #>> '{{}}')){} <= ${}{}",
                        extract_expr,
                        cast,
                        *param_idx,
                        cast,
                        extract_expr,
                        cast,
                        *param_idx + 1,
                        cast
                    );
                    binds.push(bind_min);
                    binds.push(bind_max);
                    *param_idx += 2;
                    s
                }
                CmpOp::Unequal => {
                    let (cast_max_actual, _) = scalar_to_sql_bind(max);
                    let s = format!(
                        "(({} #>> '{{}}')){} < ${}{} OR (({} #>> '{{}}')){} > ${}{}",
                        extract_expr,
                        cast,
                        *param_idx,
                        cast,
                        extract_expr,
                        cast_max_actual,
                        *param_idx + 1,
                        cast_max_actual
                    );
                    binds.push(bind_min);
                    binds.push(bind_max);
                    *param_idx += 2;
                    s
                }
                _ => {
                    return Err(AndromedaError::bad_request_data("Range values only supported with == and != operators"));
                }
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
                    format!("(({} #>> '{{}}')){} = ${}{}", extract_expr, cast, idx, cast)
                })
                .collect();
            *param_idx += values.len();
            match op {
                CmpOp::Equal => Ok(format!("({})", parts.join(" OR "))),
                CmpOp::Unequal => Ok(format!("NOT ({})", parts.join(" OR "))),
                _ => Err(AndromedaError::bad_request_data("Value lists only supported with == and != operators")),
            }
        }
    }
}

fn cmp_op_sql(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

fn scalar_to_sql_bind(scalar: &Scalar) -> (&'static str, BindValue) {
    match scalar {
        Scalar::Number(_n) => ("::numeric", BindValue::String(_n.to_string())),
        Scalar::Bool(b) => ("::boolean", BindValue::String(b.to_string())),
        Scalar::Str(s) => ("", BindValue::String(s.clone())),
        Scalar::DateTime(s) | Scalar::Date(s) | Scalar::Time(s) => ("", BindValue::String(s.clone())),
    }
}

fn property_value_jsonpath(attr_iri: &str) -> String {
    format!(
        "$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"",
        escape_jsonpath_key(attr_iri)
    )
}

fn value_extraction_jsonpath(attr_iri: &str, sub_attrs: &[String], value_path: &[String]) -> String {
    value_extraction_jsonpath_with_predicate(attr_iri, sub_attrs, value_path, "https://uri.etsi.org/ngsi-ld/hasValue")
}

fn value_extraction_jsonpath_with_predicate(attr_iri: &str, sub_attrs: &[String], value_path: &[String], predicate_iri: &str) -> String {
    let mut path = format!("$[0].\"{}\"", escape_jsonpath_key(attr_iri));

    for sub in sub_attrs {
        path.push_str(&format!("[0].\"{}\"", escape_jsonpath_key(sub)));
    }

    path.push_str(&format!("[0].\"{}\"[0]", escape_jsonpath_key(predicate_iri)));

    let value_key = if predicate_iri == "https://uri.etsi.org/ngsi-ld/hasObject" {
        "@id"
    } else {
        "@value"
    };

    if value_path.is_empty() {
        path.push_str(&format!(".\"{}\"", value_key));
    } else {
        path.push_str(&format!(".\"{}\"", value_key));
        for key in value_path {
            path.push_str(&format!(".\"{}\"", escape_jsonpath_key(key)));
        }
    }

    path
}

fn geo_to_sql(
    geo: &crate::query::compiled::GeoQuery,
    binds: &mut Vec<BindValue>,
    param_idx: &mut usize,
    payload_column: &str,
) -> Result<String, AndromedaError> {
    // Use materialized location column for default geoproperty to leverage GIST index
    let is_default_location = geo.geoproperty_iri == "https://uri.etsi.org/ngsi-ld/location";
    let geo_extract = if is_default_location {
        // Use table alias prefix if payload_column contains a dot (e.g. "e.payload" -> "e.location")
        if let Some(prefix) = payload_column.strip_suffix(".payload") {
            format!("{}.location", prefix)
        } else {
            "location".to_string()
        }
    } else {
        format!(
            "ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first({}, '$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"')::text)), 4326)",
            payload_column,
            escape_jsonpath_key(&geo.geoproperty_iri)
        )
    };

    let geojson = build_geojson(&geo.geometry, &geo.coordinates)?;

    let ref_geom = format!("ST_SetSRID(ST_GeomFromGeoJSON(${}), 4326)", *param_idx);
    binds.push(BindValue::String(geojson));
    *param_idx += 1;

    let sql = match &geo.georel {
        GeoRel::Within => format!("ST_Within({}, {})", geo_extract, ref_geom),
        GeoRel::Contains => format!("ST_Contains({}, {})", geo_extract, ref_geom),
        GeoRel::Intersects => format!("ST_Intersects({}, {})", geo_extract, ref_geom),
        GeoRel::Disjoint => format!("ST_Disjoint({}, {})", geo_extract, ref_geom),
        GeoRel::Equals => format!("ST_Equals({}, {})", geo_extract, ref_geom),
        GeoRel::Overlaps => format!("ST_Overlaps({}, {})", geo_extract, ref_geom),
        GeoRel::Near { max_distance, min_distance } => {
            let mut parts = Vec::new();
            if let Some(max) = max_distance {
                parts.push(format!("ST_DWithin({}::geography, {}::geography, {})", geo_extract, ref_geom, max));
            }
            if let Some(min) = min_distance {
                parts.push(format!("ST_Distance({}::geography, {}::geography) >= {}", geo_extract, ref_geom, min));
            }
            if parts.is_empty() {
                return Err(AndromedaError::bad_request_data("near georel requires maxDistance or minDistance"));
            }
            parts.join(" AND ")
        }
    };

    Ok(sql)
}

fn build_geojson(geometry: &str, coordinates: &str) -> Result<String, AndromedaError> {
    let coords_value: Value =
        serde_json::from_str(coordinates).map_err(|_| AndromedaError::bad_request_data(format!("Invalid coordinates: {}", coordinates)))?;

    let geojson = serde_json::json!({
        "type": geometry,
        "coordinates": coords_value
    });

    Ok(geojson.to_string())
}

fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the SQL expression to extract a geometry from an entity for spatial operations.
/// Uses the materialized `location` column for the default geoproperty (GIST-indexed),
/// or extracts from the JSONB payload for custom geoproperties.
fn build_geo_extract(geoproperty_iri: &str, payload_column: &str) -> String {
    let is_default_location = geoproperty_iri == "https://uri.etsi.org/ngsi-ld/location";
    if is_default_location {
        if let Some(prefix) = payload_column.strip_suffix(".payload") {
            format!("{}.location", prefix)
        } else {
            "location".to_string()
        }
    } else {
        format!(
            "ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first({}, '$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"')::text)), 4326)",
            payload_column,
            escape_jsonpath_key(geoproperty_iri)
        )
    }
}

fn scope_q_to_sql(scope_q: &str, binds: &mut Vec<BindValue>, param_idx: &mut usize) -> Result<String, AndromedaError> {
    // NGSI-LD: comma is OR, semicolon is AND (higher precedence)
    // e.g. "/A;/B,/C" → (/A AND /B) OR /C
    let or_terms: Vec<&str> = scope_q.split(',').map(str::trim).collect();
    let mut or_parts = Vec::new();
    for or_term in &or_terms {
        let and_terms: Vec<&str> = or_term.split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            let part = single_scope_to_sql(and_terms[0], binds, param_idx)?;
            or_parts.push(part);
        } else {
            let mut and_parts = Vec::new();
            for and_term in &and_terms {
                let part = single_scope_to_sql(and_term, binds, param_idx)?;
                and_parts.push(part);
            }
            or_parts.push(format!("({})", and_parts.join(" AND ")));
        }
    }
    if or_parts.len() == 1 {
        Ok(or_parts.into_iter().next().unwrap())
    } else {
        Ok(format!("({})", or_parts.join(" OR ")))
    }
}

fn single_scope_to_sql(scope: &str, binds: &mut Vec<BindValue>, param_idx: &mut usize) -> Result<String, AndromedaError> {
    if let Some(prefix) = scope.strip_suffix("/#") {
        // Hash wildcard: match one or more levels below the prefix
        let regex = format!("^{}/.+$", regex::escape(prefix));
        let sql = format!("EXISTS(SELECT 1 FROM unnest(scopes) s WHERE s ~ ${})", *param_idx);
        binds.push(BindValue::String(regex));
        *param_idx += 1;
        Ok(sql)
    } else if scope.contains("/+") {
        // Plus wildcard: match any single component at that position
        let regex = format!("^{}$", scope.replace("/+", "/[^/]+"));
        let sql = format!("EXISTS(SELECT 1 FROM unnest(scopes) s WHERE s ~ ${})", *param_idx);
        binds.push(BindValue::String(regex));
        *param_idx += 1;
        Ok(sql)
    } else {
        // Exact match
        let sql = format!("${} = ANY(scopes)", *param_idx);
        binds.push(BindValue::String(scope.to_string()));
        *param_idx += 1;
        Ok(sql)
    }
}

/// Build entity filter conditions for use in a JOIN (e.g. temporal Stage 1).
/// Uses `table_prefix` for column references (e.g. "e" → "e.id", "e.types", "e.payload").
pub struct EntityFilterConditionsInput<'a> {
    pub ids: Option<&'a [String]>,
    pub id_pattern: Option<&'a str>,
    pub types: Option<&'a [String]>,
    pub q_expr: Option<&'a Expr>,
    pub geo: Option<&'a GeoQuery>,
    pub table_prefix: &'a str,
    pub binds: &'a mut Vec<BindValue>,
    pub param_idx: &'a mut usize,
}

pub fn build_entity_filter_conditions(input: EntityFilterConditionsInput<'_>) -> Result<Vec<String>, AndromedaError> {
    let EntityFilterConditionsInput {
        ids,
        id_pattern,
        types,
        q_expr,
        geo,
        table_prefix,
        binds,
        param_idx,
    } = input;

    let mut conditions = Vec::new();
    let id_col = format!("{}.id", table_prefix);
    let types_col = format!("{}.types", table_prefix);
    let payload_col = format!("{}.payload", table_prefix);

    if let Some(ids) = ids
        && !ids.is_empty()
    {
        conditions.push(format!("{} = ANY(${})", id_col, *param_idx));
        binds.push(BindValue::StringVec(ids.to_vec()));
        *param_idx += 1;
    }

    if let Some(pattern) = id_pattern {
        conditions.push(format!("{} ~ ${}", id_col, *param_idx));
        binds.push(BindValue::String(pattern.to_string()));
        *param_idx += 1;
    }

    if let Some(types) = types
        && !types.is_empty()
    {
        if types.len() == 1 {
            conditions.push(format!("${} = ANY({})", *param_idx, types_col));
            binds.push(BindValue::String(types[0].clone()));
        } else {
            conditions.push(format!("{} && ${}", types_col, *param_idx));
            binds.push(BindValue::StringVec(types.to_vec()));
        }
        *param_idx += 1;
    }

    if let Some(expr) = q_expr {
        let q_sql = expr_to_sql(expr, binds, param_idx, &payload_col)?;
        conditions.push(q_sql);
    }

    if let Some(geo) = geo {
        let geo_sql = geo_to_sql(geo, binds, param_idx, &payload_col)?;
        conditions.push(geo_sql);
    }

    Ok(conditions)
}

pub fn build_ids_query(query: &CompiledEntityQuery) -> Result<BuiltQuery, AndromedaError> {
    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<BindValue> = Vec::new();
    let mut param_idx = 1usize;

    if let Some(ids) = &query.ids {
        conditions.push(format!("id = ANY(${})", param_idx));
        binds.push(BindValue::StringVec(ids.clone()));
        param_idx += 1;
    }

    if let Some(pattern) = &query.id_pattern {
        conditions.push(format!("id ~ ${}", param_idx));
        binds.push(BindValue::String(pattern.clone()));
        param_idx += 1;
    }

    if let Some(types) = &query.types {
        if types.len() == 1 {
            conditions.push(format!("${} = ANY(types)", param_idx));
            binds.push(BindValue::String(types[0].clone()));
            param_idx += 1;
        } else if query.type_match_all {
            for t in types {
                conditions.push(format!("${} = ANY(types)", param_idx));
                binds.push(BindValue::String(t.clone()));
                param_idx += 1;
            }
        } else {
            conditions.push(format!("types && ${}", param_idx));
            binds.push(BindValue::StringVec(types.clone()));
            param_idx += 1;
        }
    }

    if !query.attrs.is_empty() {
        let attr_conditions: Vec<String> = query
            .attrs
            .iter()
            .map(|attr| {
                let cond = format!("payload->0 ? ${}", param_idx);
                binds.push(BindValue::String(attr.clone()));
                param_idx += 1;
                cond
            })
            .collect();
        conditions.push(format!("({})", attr_conditions.join(" OR ")));
    }

    if let Some(expr) = &query.q_expr {
        let q_sql = expr_to_sql(expr, &mut binds, &mut param_idx, "payload")?;
        conditions.push(q_sql);
    }

    if let Some(geo) = &query.geo {
        let geo_sql = geo_to_sql(geo, &mut binds, &mut param_idx, "payload")?;
        conditions.push(geo_sql);
    }

    if let Some(scope_q) = &query.scope_q {
        let scope_sql = scope_q_to_sql(scope_q, &mut binds, &mut param_idx)?;
        conditions.push(scope_sql);
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let order_clause = if query.order_by.is_empty() {
        " ORDER BY id ASC".to_string()
    } else {
        let order_parts: Vec<String> = query
            .order_by
            .iter()
            .map(|term| {
                let dir = term.direction.sql_dir();
                if term.direction.is_distance() {
                    if let Some(geo) = &query.geo {
                        let geo_extract = build_geo_extract(&geo.geoproperty_iri, "payload");
                        let ref_geom = format!("ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326)", build_geojson(&geo.geometry, &geo.coordinates)?);
                        Ok(format!("ST_Distance({}::geography, {}::geography) {} NULLS LAST", geo_extract, ref_geom, dir))
                    } else {
                        let json_path = property_value_jsonpath(&term.attr_iri);
                        Ok(format!("jsonb_path_query_first(payload, '{}') {} NULLS LAST", json_path, dir))
                    }
                } else {
                    let json_path = property_value_jsonpath(&term.attr_iri);
                    let collation_clause = term.collation.as_ref().map(|c| format!(" COLLATE \"{}\"", c)).unwrap_or_default();
                    Ok(format!(
                        "jsonb_path_query_first(payload, '{}'){} {} NULLS LAST",
                        json_path, collation_clause, dir
                    ))
                }
            })
            .collect::<Result<Vec<_>, AndromedaError>>()?;
        format!(" ORDER BY {}, id ASC", order_parts.join(", "))
    };

    let pagination = format!(" LIMIT {} OFFSET {}", query.limit, query.offset);

    let sql = format!("SELECT id FROM entities{}{}{}", where_clause, order_clause, pagination);

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values: binds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm},
        entity::OrderDirection,
    };

    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery {
            ids: None,
            id_pattern: None,
            types: None,
            attrs: Vec::new(),
            q_expr: None,
            geo: None,
            limit: 20,
            offset: 0,
            count: false,
            pick: Vec::new(),
            omit: Vec::new(),
            order_by: Vec::new(),
            scope_q: None,
            type_match_all: false,
        }
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build_query(&q).unwrap();
        assert_eq!(built.sql, "SELECT payload FROM entities LIMIT 20 OFFSET 0");
        assert!(built.count_sql.is_none());
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = Some(vec!["urn:a".to_string(), "urn:b".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("id = ANY($1)"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some("urn:ngsi-ld:Vehicle:.*".to_string());
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("id ~ $1"));
    }

    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.types = Some(vec!["https://example.com/Vehicle".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("$1 = ANY(types)"));
    }

    #[test]
    fn build_with_multiple_types() {
        let mut q = empty_query();
        q.types = Some(vec!["TypeA".to_string(), "TypeB".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("types && $1"));
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.count = true;
        q.types = Some(vec!["Vehicle".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().starts_with("SELECT COUNT(*)"));
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(crate::query::q::parse("temperature").unwrap());
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("payload->0 ? $1"));
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        let expr = Expr::Pred(Predicate::Compare {
            path: crate::query::q::AttrPath::simple("https://example.com/temperature"),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        });
        cq.q_expr = Some(expr);
        let built = build_query(&cq).unwrap();
        assert!(built.sql.contains("jsonb_path_query_first"));
        assert!(built.sql.contains(">"));
    }

    #[test]
    fn build_with_limit_offset() {
        let mut q = empty_query();
        q.limit = 50;
        q.offset = 100;
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("LIMIT 50 OFFSET 100"));
    }

    #[test]
    fn build_with_order_by_asc() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://example.com/name".to_string(),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ORDER BY"));
        assert!(built.sql.contains("ASC NULLS LAST"));
        assert!(built.sql.contains("hasValue"));
    }

    #[test]
    fn build_with_order_by_collation() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://example.com/name".to_string(),
            direction: OrderDirection::Asc,
            collation: Some("und-u-ks-identic".to_string()),
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("COLLATE \"und-u-ks-identic\""));
        assert!(built.sql.contains("ASC NULLS LAST"));
    }

    #[test]
    fn build_with_order_by_dist_asc_with_geo() {
        use crate::query::compiled::{GeoQuery, GeoRel};
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: "[13.4,52.5]".to_string(),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "location".to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC NULLS LAST"));
    }

    #[test]
    fn build_with_order_by_dist_desc_with_geo() {
        use crate::query::compiled::{GeoQuery, GeoRel};
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(5000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: "[2.3,48.8]".to_string(),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "location".to_string(),
            direction: OrderDirection::DistDesc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("DESC NULLS LAST"));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_fallback() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "location".to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        // No geo query: falls back to regular attribute ordering
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("jsonb_path_query_first"));
        assert!(!built.sql.contains("ST_Distance"));
    }

    #[test]
    fn value_extraction_jsonpath_simple() {
        let jp = value_extraction_jsonpath("https://example.com/temperature", &[], &[]);
        assert!(jp.contains("hasValue"));
        assert!(jp.contains("@value"));
    }

    #[test]
    fn value_extraction_jsonpath_with_compound() {
        let jp = value_extraction_jsonpath("https://example.com/address", &[], &["city".to_string()]);
        assert!(jp.contains("@value"));
        assert!(jp.contains("city"));
    }

    #[test]
    fn value_extraction_jsonpath_with_sub_attr() {
        let jp = value_extraction_jsonpath("https://example.com/temperature", &["https://uri.etsi.org/ngsi-ld/observedAt".to_string()], &[]);
        assert!(jp.contains("observedAt"));
    }
}
