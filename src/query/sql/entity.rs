//! SQL generation for entity queries using sea-query.

use crate::{
    core::error::AndromedaError,
    jsonld::vocab,
    query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery, GeoRel, OrderDirection, OrderingGeometry, TypeExpr},
        q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
        sql::{
            BindValue,
            BuiltQuery,
            postgres_ext::{
                any_eq,
                jsonb_key_exists,
                ltree_matches,
                regex_match,
                st_contains,
                st_disjoint,
                st_dwithin,
                st_equals,
                st_intersects,
                st_overlaps,
                st_within,
                validated_geom_from_geojson,
            },
            schema::Entities,
            scope_to_lquery,
        },
    },
};
use sea_query::{Asterisk, Condition, Expr as SeaExpr, Func, Order, PostgresQueryBuilder, Query, SelectStatement};
use std::borrow::Cow;

/// Builds a parameterized SQL query to retrieve entity payloads from the `entities` table.
///
/// Generates filter conditions from the compiled query (IDs, types, attrs, q expression,
/// geo filter, scope query), ordering, and pagination clauses.
pub fn build_query(query: &CompiledEntityQuery) -> Result<BuiltQuery<'static>, AndromedaError> {
    let condition = build_entity_conditions(query)?;

    let mut select = Query::select();
    select.column(Entities::Payload).from(Entities::Table).cond_where(condition.clone());

    // When a total count is requested, piggyback it on every row as
    // `COUNT(*) OVER ()` so the common case runs in a single round-trip.
    // The count column is constant across the result set (PostgreSQL evaluates
    // the unbounded partition once), so the repository reads it from any row.
    // A zero-row result still needs the fallback `count_sql` because the
    // window function returns no rows when the filter matches nothing.
    if query.pagination.count {
        select.expr_as(SeaExpr::cust("COUNT(*) OVER ()"), sea_query::Alias::new(TOTAL_COUNT_COLUMN));
    }

    select.limit(query.pagination.limit as u64).offset(query.pagination.offset as u64);

    // Add ORDER BY clause
    add_order_clause(&mut select, &query.order_by, query.geo.as_ref(), query.ordering_geometry.as_ref())?;

    // Build main query
    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    // Fallback count query covers the zero-row edge case described above.
    let count_sql = if query.pagination.count {
        let mut count_select = Query::select();
        count_select
            .expr(Func::count(SeaExpr::col(Asterisk)))
            .from(Entities::Table)
            .cond_where(condition);
        let (count_sql_str, _) = count_select.build(PostgresQueryBuilder);
        Some(count_sql_str)
    } else {
        None
    };

    Ok(BuiltQuery { sql, count_sql, bind_values })
}

/// Column name used for the window-function count attached to each row of a
/// paginated entity query when `count=true`. Kept in sync with the repository
/// read path in [`crate::repository::entity::repo::EntityRepository::query_compiled`].
pub const TOTAL_COUNT_COLUMN: &str = "total_count";

/// Builds a parameterized SQL query to retrieve only entity IDs from the `entities` table.
///
/// Similar to [`build_query`] but selects `id` instead of `payload`, used for
/// operations that only need to identify matching entities.
pub fn build_ids_query(query: &CompiledEntityQuery) -> Result<BuiltQuery<'static>, AndromedaError> {
    let condition = build_entity_conditions(query)?;

    let mut select = Query::select();
    select.column(Entities::Id).from(Entities::Table).cond_where(condition);

    // Add ORDER BY clause (default to id ASC if none specified)
    if query.order_by.is_empty() {
        select.order_by(Entities::Id, Order::Asc);
    } else {
        add_order_clause(&mut select, &query.order_by, query.geo.as_ref(), query.ordering_geometry.as_ref())?;
    }

    select.limit(query.pagination.limit as u64).offset(query.pagination.offset as u64);

    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values,
    })
}

/// Builds sea-query `Condition` from compiled entity query filters.
fn build_entity_conditions(query: &CompiledEntityQuery) -> Result<Condition, AndromedaError> {
    let mut condition = Condition::all();

    // ID filter: id = ANY($N)
    if let Some(ids) = &query.ids
        && !ids.is_empty()
    {
        condition = condition.add(SeaExpr::col(Entities::Id).is_in(ids.clone()));
    }

    // ID pattern filter: id ~ $N
    if let Some(pattern) = &query.id_pattern {
        condition = condition.add(regex_match(SeaExpr::col(Entities::Id).into(), SeaExpr::val(pattern.as_str()).into()));
    }

    // Type expression filter
    if let Some(type_expr) = &query.type_expr {
        let type_cond = type_expr_to_condition(type_expr);
        condition = condition.add(type_cond);
    }

    // Attribute existence filter
    if !query.attrs.is_empty() {
        let mut attr_cond = Condition::any();
        for attr in &query.attrs {
            attr_cond = attr_cond.add(jsonb_key_exists(SeaExpr::cust("\"payload\"->0"), SeaExpr::val(attr.as_str()).into()));
        }
        condition = condition.add(attr_cond);
    }

    // Q expression filter
    if let Some(q_expr) = &query.q_expr {
        let q_cond = q_expr_to_condition(q_expr, "payload")?;
        condition = condition.add(q_cond);
    }

    // Geo filter
    if let Some(geo) = &query.geo {
        let geo_cond = geo_to_condition(geo, "payload")?;
        condition = condition.add(geo_cond);
    }

    // Scope filter
    if let Some(scope_q) = &query.scope_q {
        let scope_cond = scope_q_to_condition(scope_q.as_str(), "scope_paths")?;
        condition = condition.add(scope_cond);
    }

    Ok(condition)
}

/// Converts TypeExpr to sea-query Condition.
fn type_expr_to_condition(expr: &TypeExpr) -> Condition {
    match expr {
        TypeExpr::Type(iri) => {
            // $N = ANY(types)
            Condition::all().add(any_eq(SeaExpr::val(iri.as_str()).into(), SeaExpr::col(Entities::Types).into()))
        }
        TypeExpr::And(children) => {
            let mut cond = Condition::all();
            for child in children {
                cond = cond.add(type_expr_to_condition(child));
            }
            cond
        }
        TypeExpr::Or(children) => {
            let mut cond = Condition::any();
            for child in children {
                cond = cond.add(type_expr_to_condition(child));
            }
            cond
        }
    }
}

/// Converts Q expression (NGSI-LD query language) to sea-query Condition.
fn q_expr_to_condition(expr: &Expr, payload_col: &str) -> Result<Condition, AndromedaError> {
    match expr {
        Expr::And(terms) => {
            let mut cond = Condition::all();
            for term in terms {
                cond = cond.add(q_expr_to_condition(term, payload_col)?);
            }
            Ok(cond)
        }
        Expr::Or(terms) => {
            let mut cond = Condition::any();
            for term in terms {
                cond = cond.add(q_expr_to_condition(term, payload_col)?);
            }
            Ok(cond)
        }
        Expr::Pred(pred) => predicate_to_condition(pred, payload_col),
    }
}

/// Converts a predicate to sea-query condition.
fn predicate_to_condition(pred: &Predicate, payload_col: &str) -> Result<Condition, AndromedaError> {
    match pred {
        Predicate::Exists(path) => {
            // payload->0 ? $N
            Ok(Condition::all().add(jsonb_key_exists(
                SeaExpr::cust(format!("\"{}\"->0", payload_col)),
                SeaExpr::val(path.attr.as_str()).into(),
            )))
        }
        Predicate::Compare { path, op, rhs } => compare_to_condition(path, *op, rhs, payload_col),
        Predicate::MatchPattern { path, pattern } => {
            let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
            // jsonb_path_exists(payload, 'path') AND (jsonb_path_query_first(payload, 'path')#>>'{}')::text ~ $N
            let sql = format!(
                "jsonb_path_exists(\"{}\", '{}') AND (jsonb_path_query_first(\"{}\", '{}')#>>'{{}}')::text ~ ",
                payload_col,
                escape_jsonpath(&jsonpath),
                payload_col,
                escape_jsonpath(&jsonpath)
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(format!("{} $1", sql), [pattern.clone()])))
        }
        Predicate::NotMatchPattern { path, pattern } => {
            let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
            let sql = format!(
                "NOT (jsonb_path_exists(\"{}\", '{}') AND (jsonb_path_query_first(\"{}\", '{}')#>>'{{}}')::text ~ ",
                payload_col,
                escape_jsonpath(&jsonpath),
                payload_col,
                escape_jsonpath(&jsonpath)
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(format!("{} $1)", sql), [pattern.clone()])))
        }
    }
}

/// Converts comparison predicate to sea-query condition.
fn compare_to_condition(path: &AttrPath, op: CmpOp, rhs: &CompareValue, payload_col: &str) -> Result<Condition, AndromedaError> {
    let jsonpath = value_extraction_jsonpath(&path.attr, &path.sub_attrs, &path.value_path);
    let has_object_jsonpath = value_extraction_jsonpath_with_predicate(&path.attr, &path.sub_attrs, &path.value_path, vocab::expanded::OBJECT.as_str());

    let extract_expr = format!("jsonb_path_query_first(\"{}\", '{}')", payload_col, escape_jsonpath(&jsonpath));
    let extract_object = format!("jsonb_path_query_first(\"{}\", '{}')", payload_col, escape_jsonpath(&has_object_jsonpath));

    match rhs {
        CompareValue::Scalar(scalar) => {
            let op_str = cmp_op_sql(op);
            let (cast, value) = scalar_to_value(scalar);
            // COALESCE((extract #>> '{}'), (extract_object #>> '{}'))::cast op $N::cast
            let sql = format!(
                "(COALESCE(({} #>> '{{}}'), ({} #>> '{{}}')){} {} $1{})",
                extract_expr, extract_object, cast, op_str, cast
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [value])))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_val) = scalar_to_value(min);
            let (_, max_val) = scalar_to_value(max);
            match op {
                CmpOp::Equal => {
                    let sql = format!(
                        "(({} #>> '{{}}')){} >= $1{} AND (({} #>> '{{}}')){} <= $2{}",
                        extract_expr, cast, cast, extract_expr, cast, cast
                    );
                    Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [min_val, max_val])))
                }
                CmpOp::Unequal => {
                    let sql = format!(
                        "(({} #>> '{{}}')){} < $1{} OR (({} #>> '{{}}')){} > $2{}",
                        extract_expr, cast, cast, extract_expr, cast, cast
                    );
                    Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [min_val, max_val])))
                }
                _ => Err(AndromedaError::query_validation("Range values only supported with == and != operators")),
            }
        }
        CompareValue::ValueList(values) => {
            let mut parts = Vec::with_capacity(values.len());
            let mut bind_values = Vec::with_capacity(values.len());
            for (i, v) in values.iter().enumerate() {
                let (cast, val) = scalar_to_value(v);
                parts.push(format!("(({} #>> '{{}}')){} = ${}{}", extract_expr, cast, i + 1, cast));
                bind_values.push(val);
            }
            let joined = parts.join(" OR ");
            let sql = match op {
                CmpOp::Equal => format!("({})", joined),
                CmpOp::Unequal => format!("NOT ({})", joined),
                _ => return Err(AndromedaError::query_validation("Value lists only supported with == and != operators")),
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, bind_values)))
        }
    }
}

/// Converts GeoQuery to sea-query condition using PostGIS helpers.
fn geo_to_condition(geo: &GeoQuery, payload_col: &str) -> Result<Condition, AndromedaError> {
    let is_default_location = geo.geoproperty_iri == vocab::expanded::LOCATION.as_str();

    // Build geometry extraction expression as SimpleExpr
    let geo_extract_expr = if is_default_location {
        SeaExpr::cust("\"location\"")
    } else {
        // For custom geoproperties, extract from JSONB and validate
        let jsonpath = format!(
            "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
            escape_jsonpath_key(&geo.geoproperty_iri),
            escape_jsonpath_key(vocab::expanded::VALUE.as_str())
        );
        let extract = SeaExpr::cust(format!("jsonb_path_query_first(\"{}\", '{}')::text", payload_col, jsonpath));
        validated_geom_from_geojson(extract)
    };

    // Build reference geometry from GeoJSON using helper
    let geojson = build_geojson(&geo.geometry, &geo.coordinates)?;
    let ref_geom_expr = validated_geom_from_geojson(SeaExpr::val(&geojson).into());

    // Build spatial condition using PostGIS helpers
    let spatial_expr = match &geo.georel {
        GeoRel::Within => st_within(geo_extract_expr, ref_geom_expr),
        GeoRel::Contains => st_contains(geo_extract_expr, ref_geom_expr),
        GeoRel::Intersects => st_intersects(geo_extract_expr, ref_geom_expr),
        GeoRel::Disjoint => st_disjoint(geo_extract_expr, ref_geom_expr),
        GeoRel::Equals => st_equals(geo_extract_expr, ref_geom_expr),
        GeoRel::Overlaps => st_overlaps(geo_extract_expr, ref_geom_expr),
        GeoRel::Near { max_distance, min_distance } => {
            // Near uses geography type for accurate distance calculations
            let geo_col_sql = if is_default_location {
                "\"location_geo\"".to_string()
            } else {
                let jsonpath = format!(
                    "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
                    escape_jsonpath_key(&geo.geoproperty_iri),
                    escape_jsonpath_key(vocab::expanded::VALUE.as_str())
                );
                format!(
                    "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(jsonb_path_query_first(\"{}\", '{}')::text), 4326))::geography",
                    payload_col, jsonpath
                )
            };
            let ref_geom_geo_sql = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))::geography", geojson);

            let mut parts = Vec::new();
            if let Some(max) = max_distance {
                parts.push(st_dwithin(SeaExpr::cust(&geo_col_sql), SeaExpr::cust(&ref_geom_geo_sql), *max));
            }
            if let Some(min) = min_distance {
                // ST_Distance for minDistance check - use raw SQL since we don't have a helper
                parts.push(SeaExpr::cust(format!("ST_Distance({}, {}) >= {}", geo_col_sql, ref_geom_geo_sql, min)));
            }
            if parts.is_empty() {
                return Err(AndromedaError::query_validation("near georel requires maxDistance or minDistance"));
            }
            // Combine with AND if multiple conditions
            if parts.len() == 1 {
                parts.remove(0)
            } else {
                // Build combined AND expression
                let first = parts.remove(0);
                let second = parts.remove(0);
                let (sql1, _) = Query::select().expr(first).build(PostgresQueryBuilder);
                let (sql2, _) = Query::select().expr(second).build(PostgresQueryBuilder);
                SeaExpr::cust(format!("({} AND {})", sql1.trim_start_matches("SELECT "), sql2.trim_start_matches("SELECT ")))
            }
        }
    };

    // For default location, add expiration guard to exclude expired location attributes
    if is_default_location {
        let location_iri = vocab::expanded::LOCATION.as_str();
        let expires_at_iri = vocab::expanded::EXPIRES_AT.as_str();
        let guard_sql = format!(
            "EXISTS (\
                SELECT 1 FROM jsonb_array_elements(\"{}\"->0->'{}') AS inst(elem) \
                WHERE (inst.elem #>> '{{{},0,@value}}') IS NULL \
                   OR (inst.elem #>> '{{{},0,@value}}')::timestamptz > NOW()\
            )",
            payload_col, location_iri, expires_at_iri, expires_at_iri
        );
        Ok(Condition::all().add(spatial_expr).add(SeaExpr::cust(guard_sql)))
    } else {
        Ok(Condition::all().add(spatial_expr))
    }
}

/// Converts scope query string to sea-query condition.
fn scope_q_to_condition(scope_q: &str, scopes_col: &str) -> Result<Condition, AndromedaError> {
    // NGSI-LD: comma is OR, semicolon is AND (higher precedence)
    let or_terms: Vec<&str> = scope_q.split(',').map(str::trim).collect();
    let mut or_cond = Condition::any();

    for or_term in &or_terms {
        let and_terms: Vec<&str> = or_term.split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            let lquery = scope_to_lquery(and_terms[0]);
            or_cond = or_cond.add(ltree_matches(SeaExpr::cust(format!("\"{}\"", scopes_col)), SeaExpr::val(lquery).into()));
        } else {
            let mut and_cond = Condition::all();
            for and_term in &and_terms {
                let lquery = scope_to_lquery(and_term);
                and_cond = and_cond.add(ltree_matches(SeaExpr::cust(format!("\"{}\"", scopes_col)), SeaExpr::val(lquery).into()));
            }
            or_cond = or_cond.add(and_cond);
        }
    }

    // If only one OR term, return it directly (avoid unnecessary nesting)
    if or_terms.len() == 1 {
        let and_terms: Vec<&str> = or_terms[0].split(';').map(str::trim).collect();
        if and_terms.len() == 1 {
            let lquery = scope_to_lquery(and_terms[0]);
            return Ok(Condition::all().add(ltree_matches(SeaExpr::cust(format!("\"{}\"", scopes_col)), SeaExpr::val(lquery).into())));
        }
    }

    Ok(or_cond)
}

/// Adds ORDER BY clause to select statement.
fn add_order_clause(
    select: &mut SelectStatement,
    order_by: &[CompiledOrderTerm],
    geo: Option<&GeoQuery>,
    ordering_geometry: Option<&OrderingGeometry>,
) -> Result<(), AndromedaError> {
    if order_by.is_empty() {
        return Ok(());
    }

    for term in order_by {
        let order = match term.direction {
            OrderDirection::Asc | OrderDirection::DistAsc => Order::Asc,
            OrderDirection::Desc | OrderDirection::DistDesc => Order::Desc,
        };

        if term.direction.is_distance() {
            // Distance-based ordering
            let (geo_col, ref_geom) = if let Some(og) = ordering_geometry {
                let geojson = og
                    .geometry
                    .to_geojson()
                    .map_err(|e| AndromedaError::query_validation(format!("Failed to decode ordering geometry: {}", e)))?;
                let geojson_str =
                    serde_json::to_string(&geojson).map_err(|e| AndromedaError::query_validation(format!("Failed to serialize ordering geometry: {}", e)))?;
                let geo_col = build_geo_extract_geography(&term.attr_iri);
                let ref_geom = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))", geojson_str);
                (geo_col, ref_geom)
            } else if let Some(geo) = geo {
                let geojson = serde_json::json!({
                    "type": geo.geometry,
                    "coordinates": geo.coordinates
                });
                let geo_col = build_geo_extract_geography(&term.attr_iri);
                let ref_geom = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))", geojson);
                (geo_col, ref_geom)
            } else {
                return Err(AndromedaError::query_validation(
                    "Distance-based ordering requires either a geo-query or orderFrom parameter".to_string(),
                ));
            };

            let dist_expr = format!("ST_Distance({}, {}::geography)", geo_col, ref_geom);
            select.order_by_expr(SeaExpr::cust(dist_expr), order);
        } else {
            // Attribute value ordering
            let json_path = format!(
                "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
                escape_jsonpath_key(&term.attr_iri),
                escape_jsonpath_key(vocab::expanded::VALUE.as_str())
            );
            let collation = term.collation.as_ref().map(|c| format!(" COLLATE \"{}\"", c.as_str())).unwrap_or_default();
            let order_expr = format!("jsonb_path_query_first(\"payload\", '{}'){}", json_path, collation);
            select.order_by_expr(SeaExpr::cust(order_expr), order);
        }
    }

    // Always add id ASC as tiebreaker
    select.order_by(Entities::Id, Order::Asc);

    Ok(())
}

// Helper functions

fn value_extraction_jsonpath(attr_iri: &str, sub_attrs: &[String], value_path: &[String]) -> String {
    value_extraction_jsonpath_with_predicate(attr_iri, sub_attrs, value_path, vocab::expanded::VALUE.as_str())
}

fn value_extraction_jsonpath_with_predicate(attr_iri: &str, sub_attrs: &[String], value_path: &[String], predicate_iri: &str) -> String {
    let mut path = format!("$[0].\"{}\"", escape_jsonpath_key(attr_iri));
    for sub in sub_attrs {
        path.push_str(&format!("[0].\"{}\"", escape_jsonpath_key(sub)));
    }
    path.push_str(&format!("[0].\"{}\"[0]", escape_jsonpath_key(predicate_iri)));
    let value_key = if predicate_iri == vocab::expanded::OBJECT.as_str() { "@id" } else { "@value" };
    path.push_str(&format!(".\"{}\"", value_key));
    for key in value_path {
        path.push_str(&format!(".\"{}\"", escape_jsonpath_key(key)));
    }
    path
}

fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_jsonpath(path: &str) -> String {
    path.replace('\'', "''")
}

fn build_geojson(geometry: &str, coordinates: &serde_json::Value) -> Result<String, AndromedaError> {
    let geojson = serde_json::json!({
        "type": geometry,
        "coordinates": coordinates
    });
    Ok(geojson.to_string())
}

fn build_geo_extract_geography(geoproperty_iri: &str) -> String {
    let is_default_location = geoproperty_iri == vocab::expanded::LOCATION.as_str();
    if is_default_location {
        "\"location_geo\"".to_string()
    } else {
        format!(
            "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first(\"payload\", '$[0].\"{}\"[0].\"{}\"[0].\"@value\"')::text)), 4326))::geography",
            escape_jsonpath_key(geoproperty_iri),
            escape_jsonpath_key(vocab::expanded::VALUE.as_str())
        )
    }
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
    use crate::query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery, GeoRel, IcuCollation, IdPattern, OrderDirection, OrderingGeometry, ScopeQuery, TypeExpr},
        q::{
            ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
            parser,
        },
    };

    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery {
            ids: None,
            id_pattern: None,
            type_expr: None,
            attrs: Vec::new(),
            q_expr: None,
            geo: None,
            ordering_geometry: None,
            pagination: crate::query::compiled::Pagination::DEFAULT,
            pick: Vec::new(),
            omit: Vec::new(),
            order_by: Vec::new(),
            scope_q: None,
        }
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("SELECT"));
        assert!(built.sql.contains("\"payload\""));
        assert!(built.sql.contains("\"entities\""));
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
        assert!(built.count_sql.is_none());
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = Some(vec!["urn:a".to_string(), "urn:b".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("\"id\" IN"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(IdPattern::new("urn:ngsi-ld:Vehicle:.*").unwrap());
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("\"id\" ~"));
    }

    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.type_expr = Some(TypeExpr::Type("https://example.com/Vehicle".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("\"types\""));
    }

    #[test]
    fn build_with_multiple_types_or() {
        let mut q = empty_query();
        q.type_expr = Some(TypeExpr::Or(vec![TypeExpr::Type("TypeA".to_string()), TypeExpr::Type("TypeB".to_string())]));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("OR"));
    }

    #[test]
    fn build_with_multiple_types_and() {
        let mut q = empty_query();
        q.type_expr = Some(TypeExpr::And(vec![TypeExpr::Type("TypeA".to_string()), TypeExpr::Type("TypeB".to_string())]));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("AND"));
    }

    #[test]
    fn build_with_mixed_type_expr() {
        let mut q = empty_query();
        // (Home AND Vehicle) OR Motorhome
        q.type_expr = Some(TypeExpr::Or(vec![
            TypeExpr::And(vec![TypeExpr::Type("Home".to_string()), TypeExpr::Type("Vehicle".to_string())]),
            TypeExpr::Type("Motorhome".to_string()),
        ]));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("AND"));
        assert!(built.sql.contains("OR"));
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.pagination.count = true;
        q.type_expr = Some(TypeExpr::Type("Vehicle".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().contains("COUNT(*)"));
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(parser::parse("temperature").unwrap());
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("\"payload\"->0 ?"));
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        let expr = Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/temperature"),
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
        q.pagination.limit = 50;
        q.pagination.offset = 100;
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
        // sea-query parameterizes limit/offset as bind values
        assert!(!built.bind_values.is_empty(), "should have bind values for limit/offset");
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
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("hasValue"));
    }

    #[test]
    fn build_with_order_by_collation() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://example.com/name".to_string(),
            direction: OrderDirection::Asc,
            collation: Some(IcuCollation::new("und-u-ks-identic".to_string())),
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("COLLATE \"und-u-ks-identic\""));
        assert!(built.sql.contains("ASC"));
    }

    #[test]
    fn build_with_order_by_dist_asc_with_geo() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([13.4, 52.5]),
            geoproperty_iri: vocab::expanded::LOCATION.as_str().to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC"));
    }

    #[test]
    fn build_with_order_by_dist_desc_with_geo() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(5000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([2.3, 48.8]),
            geoproperty_iri: vocab::expanded::LOCATION.as_str().to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistDesc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("DESC"));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_returns_error() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let result = build_query(&q);
        assert!(result.is_err());
    }

    #[test]
    fn build_with_order_by_dist_with_ordering_geometry() {
        let mut q = empty_query();
        let wkb = crate::domain::geometry::Wkb::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering_geometry = Some(OrderingGeometry::new(wkb));
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("13.3986"));
    }

    #[test]
    fn build_with_order_by_dist_ordering_geometry_takes_precedence() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([2.3522, 48.8566]),
            geoproperty_iri: vocab::expanded::LOCATION.as_str().to_string(),
        });
        let wkb = crate::domain::geometry::Wkb::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering_geometry = Some(OrderingGeometry::new(wkb));
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        // Should use the ordering geometry (Berlin), not the geo query (Paris)
        assert!(built.sql.contains("13.3986"));
    }

    #[test]
    fn geo_default_location_includes_expiration_guard() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(5000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([13.4, 52.5]),
            geoproperty_iri: vocab::expanded::LOCATION.as_str().to_string(),
        });
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_DWithin"), "should contain spatial predicate");
        assert!(built.sql.contains("EXISTS"), "should contain EXISTS expiration guard");
        assert!(built.sql.contains("expiresAt"), "should reference expiresAt IRI");
        assert!(built.sql.contains("NOW()"), "should compare against NOW()");
    }

    #[test]
    fn geo_non_default_geoproperty_omits_expiration_guard() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: "Polygon".to_string(),
            coordinates: serde_json::json!([[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]]),
            geoproperty_iri: "https://example.com/customGeo".to_string(),
        });
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Within"), "should contain spatial predicate");
        assert!(!built.sql.contains("EXISTS"), "should NOT contain EXISTS guard for non-default geoproperty");
    }

    #[test]
    fn geo_default_location_within_polygon() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: "Polygon".to_string(),
            coordinates: serde_json::json!([[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]]),
            geoproperty_iri: vocab::expanded::LOCATION.as_str().to_string(),
        });
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Within"), "should contain ST_Within spatial predicate");
        assert!(matches!(built.bind_values[1], BindValue::I64(20)), "limit should be I64(20)");
        assert!(matches!(built.bind_values[2], BindValue::I64(0)), "offset should be I64(0)");
    }

    #[test]
    fn scope_exact_generates_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A/B".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("::lquery"), "SQL should cast to lquery");
    }

    #[test]
    fn scope_hash_generates_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A/#".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("::lquery"));
    }

    #[test]
    fn scope_or_generates_disjunction() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A,/B".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains(" OR "), "SQL should contain OR for comma-separated scopes");
    }

    #[test]
    fn scope_and_generates_conjunction() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A;/B".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND for semicolon-separated scopes");
    }

    #[test]
    fn scope_complex_generates_mixed() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A;/B,/C/#".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND");
        assert!(built.sql.contains(" OR "), "SQL should contain OR");
    }
}
