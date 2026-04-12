//! SQL generation from compiled query types.
//!
//! Shared types, bind helpers, and entity filter condition building used across
//! entity, temporal, and CSR SQL generators.

pub mod csr;
pub mod entity;
pub mod temporal;

use crate::{
    core::error::AndromedaError,
    domain::temporal::TimeRel,
    jsonld::vocab,
    query::{
        compiled::{GeoQuery, GeoRel, TypeExpr},
        q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
    },
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::borrow::Cow;

/// The result of SQL query generation: a parameterized SQL string plus bind values.
#[derive(Debug)]
pub struct BuiltQuery<'a> {
    /// The parameterized SQL query string with `$N` placeholders.
    pub sql: String,
    /// Optional count query for pagination (when `count=true`).
    pub count_sql: Option<String>,
    /// Bind values corresponding to the `$N` placeholders in order.
    pub bind_values: Vec<BindValue<'a>>,
}

/// Bind all values in a [`BindValue`] slice to a sqlx query.
///
/// Works with `sqlx::query()`, `sqlx::query_scalar()`, and `sqlx::query_as()` since
/// all share the same `.bind()` method signature. The query type is returned after
/// all binds are applied.
#[macro_export]
macro_rules! bind_values {
    ($query:expr, $binds:expr) => {{
        let mut q = $query;
        for bind in $binds {
            q = match bind {
                $crate::query::sql::BindValue::String(s) => q.bind(s.as_ref()),
                $crate::query::sql::BindValue::StringVec(v) => q.bind(v.as_ref()),
                $crate::query::sql::BindValue::I64(i) => q.bind(*i),
            };
        }
        q
    }};
}

/// A typed bind value for parameterized SQL queries.
#[derive(Debug, Clone)]
pub enum BindValue<'a> {
    /// A single string or text value.
    String(Cow<'a, str>),
    /// An array of strings (for `= ANY($N)` conditions).
    StringVec(Cow<'a, [String]>),
    /// A 64-bit integer value.
    I64(i64),
}

/// Convert an NGSI-LD scope path to PostgreSQL ltree format for storage.
///
/// Strips the leading `/` and replaces remaining `/` separators with `.` to form
/// a valid ltree label path.
///
/// # Examples
/// - `/A/B/C` -> `A.B.C`
/// - `/Madrid` -> `Madrid`
pub fn scope_to_ltree(scope: &str) -> String {
    scope.trim_start_matches('/').replace('/', ".")
}

/// Convert an NGSI-LD scope query pattern to PostgreSQL lquery format for matching.
///
/// Translates NGSI-LD wildcard conventions into lquery syntax:
/// - `#` (at end) becomes `*{1,}` (one or more labels)
/// - `+` becomes `*{1}` (exactly one label)
/// - Literal segments pass through unchanged.
///
/// # Examples
/// - `/A/B/C` -> `A.B.C`
/// - `/A/#` -> `A.*{1,}`
/// - `/A/+/C` -> `A.*{1}.C`
/// - `/#` -> `*{1,}`
pub fn scope_to_lquery(scope: &str) -> String {
    let trimmed = scope.trim_start_matches('/');
    trimmed
        .split('/')
        .map(|segment| match segment {
            "#" => "*{1,}",
            "+" => "*{1}",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Returns the SQL operator string for a comparison operator.
pub(crate) const fn cmp_op_sql(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

/// Converts a scalar value to its SQL cast suffix and bind value.
pub(crate) fn scalar_to_sql_bind(scalar: &Scalar) -> (&'static str, BindValue<'static>) {
    match scalar {
        Scalar::Number(n) => ("::numeric", BindValue::String(Cow::Owned(n.to_string()))),
        Scalar::Bool(b) => ("::boolean", BindValue::String(Cow::Owned(b.to_string()))),
        Scalar::Str(s) => ("", BindValue::String(Cow::Owned(s.clone()))),
        Scalar::DateTime(s) | Scalar::Date(s) | Scalar::Time(s) => ("", BindValue::String(Cow::Owned(s.clone()))),
    }
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

    path.push_str(&format!(".\"{}\"", value_key));
    for key in value_path {
        path.push_str(&format!(".\"{}\"", escape_jsonpath_key(key)));
    }

    path
}

pub(crate) fn expr_to_sql(expr: &Expr, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize, payload_column: &str) -> Result<String, AndromedaError> {
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

fn pred_to_sql(pred: &Predicate, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize, payload_column: &str) -> Result<String, AndromedaError> {
    match pred {
        Predicate::Exists(path) => {
            let attr_iri = &path.attr;
            let sql = format!("{}->0 ? ${}", payload_column, *param_idx);
            binds.push(BindValue::String(Cow::Owned(attr_iri.clone())));
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
            binds.push(BindValue::String(Cow::Owned(pattern.clone())));
            *param_idx += 1;
            Ok(sql)
        }
        Predicate::NotMatchPattern { path, pattern } => {
            let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
            let sql = format!(
                "NOT (jsonb_path_exists({}, '{}') AND (jsonb_path_query_first({}, '{}')#>>'{{}}')::text ~ ${})",
                payload_column, jsonpath, payload_column, jsonpath, *param_idx
            );
            binds.push(BindValue::String(Cow::Owned(pattern.clone())));
            *param_idx += 1;
            Ok(sql)
        }
    }
}

fn compare_to_sql(
    path: &AttrPath,
    op: CmpOp,
    rhs: &CompareValue,
    binds: &mut Vec<BindValue<'_>>,
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
                    return Err(AndromedaError::query_validation("Range values only supported with == and != operators"));
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
                _ => Err(AndromedaError::query_validation("Value lists only supported with == and != operators")),
            }
        }
    }
}

fn geo_to_sql(geo: &GeoQuery, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize, payload_column: &str) -> Result<String, AndromedaError> {
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
            "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first({}, '$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"')::text)), 4326))",
            payload_column,
            escape_jsonpath_key(&geo.geoproperty_iri)
        )
    };

    let geojson = build_geojson(&geo.geometry, &geo.coordinates)?;

    let ref_geom = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(${}), 4326))", *param_idx);
    binds.push(BindValue::String(Cow::Owned(geojson)));
    *param_idx += 1;

    let spatial_sql = match &geo.georel {
        GeoRel::Within => format!("ST_Within({}, {})", geo_extract, ref_geom),
        GeoRel::Contains => format!("ST_Contains({}, {})", geo_extract, ref_geom),
        GeoRel::Intersects => format!("ST_Intersects({}, {})", geo_extract, ref_geom),
        GeoRel::Disjoint => format!("ST_Disjoint({}, {})", geo_extract, ref_geom),
        GeoRel::Equals => format!("ST_Equals({}, {})", geo_extract, ref_geom),
        GeoRel::Overlaps => format!("ST_Overlaps({}, {})", geo_extract, ref_geom),
        GeoRel::Near { max_distance, min_distance } => {
            let geo_col = build_geo_extract_geography(&geo.geoproperty_iri, payload_column);
            let mut parts = Vec::new();
            if let Some(max) = max_distance {
                parts.push(format!("ST_DWithin({}, {}::geography, {})", geo_col, ref_geom, max));
            }
            if let Some(min) = min_distance {
                parts.push(format!("ST_Distance({}, {}::geography) >= {}", geo_col, ref_geom, min));
            }
            if parts.is_empty() {
                return Err(AndromedaError::query_validation("near georel requires maxDistance or minDistance"));
            }
            parts.join(" AND ")
        }
    };

    // When using the materialized location column, guard against expired location
    // attributes that the background worker has not yet NULLed. The EXISTS subquery
    // confirms at least one non-expired location instance exists in the JSONB payload.
    let sql = if is_default_location {
        let location_iri = vocab::LOCATION.as_str();
        let expires_at_iri = vocab::EXPIRES_AT.as_str();
        format!(
            "({spatial_sql} AND EXISTS (\
                SELECT 1 FROM jsonb_array_elements({payload_column}->0->'{location_iri}') AS inst(elem) \
                WHERE (inst.elem #>> '{{{expires_at_iri},0,@value}}') IS NULL \
                   OR (inst.elem #>> '{{{expires_at_iri},0,@value}}')::timestamptz > NOW()\
            ))"
        )
    } else {
        spatial_sql
    };

    Ok(sql)
}

fn build_geojson(geometry: &str, coordinates: &Value) -> Result<String, AndromedaError> {
    let geojson = serde_json::json!({
        "type": geometry,
        "coordinates": coordinates
    });

    Ok(geojson.to_string())
}

fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the SQL expression to extract a geometry from an entity for spatial operations.
/// Uses the materialized `location` column for the default geoproperty (GIST-indexed),
/// or extracts from the JSONB payload for custom geoproperties.
pub(crate) fn build_geo_extract(geoproperty_iri: &str, payload_column: &str) -> String {
    let is_default_location = geoproperty_iri == "https://uri.etsi.org/ngsi-ld/location";
    if is_default_location {
        if let Some(prefix) = payload_column.strip_suffix(".payload") {
            format!("{}.location", prefix)
        } else {
            "location".to_string()
        }
    } else {
        format!(
            "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first({}, '$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"')::text)), 4326))",
            payload_column,
            escape_jsonpath_key(geoproperty_iri)
        )
    }
}

/// Build the SQL expression to extract a geography-typed value for distance/near operations.
///
/// For the default geoproperty (`location`), returns the pre-computed `location_geo`
/// column (already `geography` type, no runtime cast needed). For custom geoproperties,
/// falls back to extracting from JSONB and casting to `geography`.
pub(crate) fn build_geo_extract_geography(geoproperty_iri: &str, payload_column: &str) -> String {
    let is_default_location = geoproperty_iri == "https://uri.etsi.org/ngsi-ld/location";
    if is_default_location {
        if let Some(prefix) = payload_column.strip_suffix(".payload") {
            format!("{}.location_geo", prefix)
        } else {
            "location_geo".to_string()
        }
    } else {
        format!("{}::geography", build_geo_extract(geoproperty_iri, payload_column))
    }
}

/// Recursively converts a `TypeExpr` into a SQL condition string.
///
/// - `Type(iri)` -> `$N = ANY(types_col)` (entity has this type)
/// - `And(children)` -> `(c1 AND c2 AND ...)`
/// - `Or(children)` -> `(c1 OR c2 OR ...)`
fn type_expr_to_sql(expr: &TypeExpr, types_col: &str, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize) -> String {
    match expr {
        TypeExpr::Type(iri) => {
            let sql = format!("${} = ANY({})", *param_idx, types_col);
            binds.push(BindValue::String(Cow::Owned(iri.clone())));
            *param_idx += 1;
            sql
        }
        TypeExpr::And(children) => {
            let parts: Vec<String> = children.iter().map(|c| type_expr_to_sql(c, types_col, binds, param_idx)).collect();
            format!("({})", parts.join(" AND "))
        }
        TypeExpr::Or(children) => {
            let parts: Vec<String> = children.iter().map(|c| type_expr_to_sql(c, types_col, binds, param_idx)).collect();
            format!("({})", parts.join(" OR "))
        }
    }
}

fn scope_q_to_sql(scope_q: &str, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize, scopes_col: &str) -> Result<String, AndromedaError> {
    // NGSI-LD: comma is OR, semicolon is AND (higher precedence)
    // e.g. "/A;/B,/C" -> (/A AND /B) OR /C
    let or_terms: Vec<&str> = scope_q.split(',').map(str::trim).collect();
    let mut or_parts = Vec::new();
    for or_term in &or_terms {
        let and_terms: Vec<&str> = or_term.split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            let part = single_scope_to_sql(and_terms[0], binds, param_idx, scopes_col)?;
            or_parts.push(part);
        } else {
            let mut and_parts = Vec::new();
            for and_term in &and_terms {
                let part = single_scope_to_sql(and_term, binds, param_idx, scopes_col)?;
                and_parts.push(part);
            }
            or_parts.push(format!("({})", and_parts.join(" AND ")));
        }
    }
    if or_parts.len() == 1 {
        // Safety: length is checked above; unwrap_or_default as a defensive fallback.
        Ok(or_parts.into_iter().next().unwrap_or_default())
    } else {
        Ok(format!("({})", or_parts.join(" OR ")))
    }
}

fn single_scope_to_sql(scope: &str, binds: &mut Vec<BindValue<'_>>, param_idx: &mut usize, scopes_col: &str) -> Result<String, AndromedaError> {
    let lquery = scope_to_lquery(scope);
    let sql = format!("{} ~ ${}::lquery", scopes_col, *param_idx);
    binds.push(BindValue::String(Cow::Owned(lquery)));
    *param_idx += 1;
    Ok(sql)
}

/// Generate a SQL condition that filters entities by historical scope presence
/// in the `scope_history` table, bounded by the temporal query's time window.
///
/// Unlike `scope_q_to_sql` which filters against the current `entities.scope_paths`
/// column, this produces an `EXISTS` subquery against `scope_history` to check
/// whether the entity had a matching scope at any point within the temporal range.
/// Per NGSI-LD 4.18, temporal evolution scopes are "considered valid from the time
/// it has been set until the time it has been explicitly removed."
pub fn scope_q_to_temporal_sql(
    scope_q: &str,
    entity_id_col: &str,
    timerel: Option<&TimeRel>,
    time_at: Option<&DateTime<Utc>>,
    end_time_at: Option<&DateTime<Utc>>,
    binds: &mut Vec<BindValue<'_>>,
    param_idx: &mut usize,
) -> Result<String, AndromedaError> {
    // Parse the scopeQ expression into AND/OR terms (same grammar as entity scopeQ)
    let or_terms: Vec<&str> = scope_q.split(',').map(str::trim).collect();
    let mut or_parts = Vec::new();
    for or_term in &or_terms {
        let and_terms: Vec<&str> = or_term.split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            let part = single_scope_to_temporal_sql(and_terms[0], entity_id_col, timerel, time_at, end_time_at, binds, param_idx)?;
            or_parts.push(part);
        } else {
            let mut and_parts = Vec::new();
            for and_term in &and_terms {
                let part = single_scope_to_temporal_sql(and_term, entity_id_col, timerel, time_at, end_time_at, binds, param_idx)?;
                and_parts.push(part);
            }
            or_parts.push(format!("({})", and_parts.join(" AND ")));
        }
    }
    if or_parts.len() == 1 {
        Ok(or_parts.into_iter().next().unwrap_or_default())
    } else {
        Ok(format!("({})", or_parts.join(" OR ")))
    }
}

/// Generate an `EXISTS` subquery for a single scope pattern against `scope_history`.
fn single_scope_to_temporal_sql(
    scope: &str,
    entity_id_col: &str,
    timerel: Option<&TimeRel>,
    time_at: Option<&DateTime<Utc>>,
    end_time_at: Option<&DateTime<Utc>>,
    binds: &mut Vec<BindValue<'_>>,
    param_idx: &mut usize,
) -> Result<String, AndromedaError> {
    let lquery = scope_to_lquery(scope);

    let scope_param = *param_idx;
    binds.push(BindValue::String(Cow::Owned(lquery)));
    *param_idx += 1;

    let mut time_conditions = Vec::new();
    if let (Some(timerel), Some(time_at)) = (timerel, time_at) {
        match timerel {
            TimeRel::Before => {
                time_conditions.push(format!("sh.time < ${}::timestamptz", *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
            }
            TimeRel::After => {
                time_conditions.push(format!("sh.time >= ${}::timestamptz", *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
            }
            TimeRel::Between => {
                time_conditions.push(format!("sh.time >= ${}::timestamptz", *param_idx));
                binds.push(BindValue::String(Cow::Owned(time_at.to_rfc3339())));
                *param_idx += 1;
                if let Some(end_time) = end_time_at {
                    time_conditions.push(format!("sh.time < ${}::timestamptz", *param_idx));
                    binds.push(BindValue::String(Cow::Owned(end_time.to_rfc3339())));
                    *param_idx += 1;
                }
            }
        }
    }

    let time_clause = if time_conditions.is_empty() {
        String::new()
    } else {
        format!(" AND {}", time_conditions.join(" AND "))
    };

    Ok(format!(
        "EXISTS (SELECT 1 FROM scope_history sh WHERE sh.entity_id = {} AND sh.scope_path ~ ${}::lquery{})",
        entity_id_col, scope_param, time_clause
    ))
}

/// Input parameters for building shared entity filter SQL conditions.
///
/// Used by the entity query builder, the entity IDs query builder, and the
/// temporal Stage 1 query builder to generate WHERE clause conditions for
/// entity-level filtering (IDs, types, attrs, q expression, geo filter,
/// scope query). The `table_prefix` controls column qualification: when
/// non-empty (e.g. `"e"`), columns are qualified as `e.id`, `e.types`,
/// `e.payload`, `e.scope_paths`; when empty, bare column names are used.
pub struct EntityFilterConditionsInput<'q, 'b, 't> {
    /// Entity IDs to filter by.
    pub ids: Option<&'q [String]>,
    /// Regex pattern for matching entity IDs.
    pub id_pattern: Option<&'q str>,
    /// Boolean expression over entity type IRIs (S4.17).
    /// Uses a separate lifetime because the expression may be constructed locally
    /// from a flat type list (e.g. in temporal queries) rather than borrowed from
    /// the compiled query.
    pub type_expr: Option<&'t TypeExpr>,
    /// Parsed `q` expression for property-level filtering.
    pub q_expr: Option<&'q Expr>,
    /// Geo-spatial query filter.
    pub geo: Option<&'q GeoQuery>,
    /// Expanded attribute IRIs to filter by (checks payload key existence).
    pub attrs: &'q [String],
    /// Scope query string (comma = OR, semicolon = AND).
    pub scope_q: Option<&'q str>,
    /// SQL table alias prefix for column references (empty for bare columns).
    pub table_prefix: &'q str,
    /// Mutable reference to the bind values vector (appended to during condition building).
    pub binds: &'b mut Vec<BindValue<'q>>,
    /// Mutable reference to the current parameter index (incremented for each bind).
    pub param_idx: &'b mut usize,
}

/// Builds shared entity filter SQL conditions from the given input parameters.
///
/// Returns a vector of SQL condition strings that can be joined with `AND`
/// for a WHERE clause. Bind values and parameter indices are updated in-place.
/// When `table_prefix` is empty, bare column names (`id`, `types`, `payload`,
/// `scope_paths`) are used; otherwise they are qualified (e.g. `e.id`).
pub fn build_entity_filter_conditions(input: EntityFilterConditionsInput<'_, '_, '_>) -> Result<Vec<String>, AndromedaError> {
    let EntityFilterConditionsInput {
        ids,
        id_pattern,
        type_expr,
        q_expr,
        geo,
        attrs,
        scope_q,
        table_prefix,
        binds,
        param_idx,
    } = input;

    let mut conditions = Vec::with_capacity(8);

    let (id_col, types_col, payload_col, scopes_col) = if table_prefix.is_empty() {
        ("id".to_string(), "types".to_string(), "payload".to_string(), "scope_paths".to_string())
    } else {
        (
            format!("{}.id", table_prefix),
            format!("{}.types", table_prefix),
            format!("{}.payload", table_prefix),
            format!("{}.scope_paths", table_prefix),
        )
    };

    if let Some(ids) = ids
        && !ids.is_empty()
    {
        conditions.push(format!("{} = ANY(${})", id_col, *param_idx));
        binds.push(BindValue::StringVec(Cow::Borrowed(ids)));
        *param_idx += 1;
    }

    if let Some(pattern) = id_pattern {
        conditions.push(format!("{} ~ ${}", id_col, *param_idx));
        binds.push(BindValue::String(Cow::Borrowed(pattern)));
        *param_idx += 1;
    }

    if let Some(expr) = type_expr {
        let sql = type_expr_to_sql(expr, &types_col, binds, param_idx);
        conditions.push(sql);
    }

    if !attrs.is_empty() {
        let attr_conditions: Vec<String> = attrs
            .iter()
            .map(|attr| {
                let cond = format!("{}->0 ? ${}", payload_col, *param_idx);
                binds.push(BindValue::String(Cow::Borrowed(attr.as_str())));
                *param_idx += 1;
                cond
            })
            .collect();
        conditions.push(format!("({})", attr_conditions.join(" OR ")));
    }

    if let Some(expr) = q_expr {
        let q_sql = expr_to_sql(expr, binds, param_idx, &payload_col)?;
        conditions.push(q_sql);
    }

    if let Some(geo) = geo {
        let geo_sql = geo_to_sql(geo, binds, param_idx, &payload_col)?;
        conditions.push(geo_sql);
    }

    if let Some(scope_q) = scope_q {
        let scope_sql = scope_q_to_sql(scope_q, binds, param_idx, &scopes_col)?;
        conditions.push(scope_sql);
    }

    Ok(conditions)
}
