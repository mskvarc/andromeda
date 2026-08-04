//! SQL generation for entity queries using sea-query.

use crate::{
    core::error::AndromedaError,
    domain::{geometry::Geometry, vocab},
    query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery, GeoRel, OrderDirection, OrderingGeometry, TypeExpr},
        sql::{
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
                validated_geom_from_ewkb,
                validated_geom_from_geojson,
            },
            q_filter::{self, QFilterTarget, ResolvedAttributeNames},
            schema::Entities,
            scope_to_lquery,
            sea_values_to_bind_values,
        },
    },
    repository::geometry::{Ewkb, encode},
};
use sea_query::{Asterisk, Condition, Expr as SeaExpr, Func, Order, PostgresQueryBuilder, Query, SelectStatement};

/// Builds a parameterized SQL query to retrieve entity payloads from the `entities` table.
///
/// Generates filter conditions from the compiled query (IDs, types, attrs, q expression,
/// geo filter, scope query), ordering, and pagination clauses.
pub fn build_query(query: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<BuiltQuery<'static>, AndromedaError> {
    let condition = build_entity_conditions(query, attribute_names)?;

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
pub fn build_ids_query(query: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<BuiltQuery<'static>, AndromedaError> {
    let condition = build_entity_conditions(query, attribute_names)?;

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
fn build_entity_conditions(query: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<Condition, AndromedaError> {
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
        condition = condition.add(q_filter::compile(q_expr, QFilterTarget::Entities, attribute_names)?);
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

/// Converts GeoQuery to sea-query condition using PostGIS helpers.
fn geo_to_condition(geo: &GeoQuery, payload_col: &str) -> Result<Condition, AndromedaError> {
    let is_default_location = geo.geoproperty_iri == vocab::expanded::properties::LOCATION.as_str();

    // Build geometry extraction expression as SimpleExpr
    let geo_extract_expr = if is_default_location {
        SeaExpr::cust("\"location\"")
    } else {
        // For custom geoproperties, extract from JSONB and validate
        let jsonpath = format!(
            "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
            escape_jsonpath_key(&geo.geoproperty_iri),
            escape_jsonpath_key(vocab::expanded::properties::VALUE.as_str())
        );
        let extract = SeaExpr::cust(format!("jsonb_path_query_first(\"{}\", '{}')::text", payload_col, jsonpath));
        validated_geom_from_geojson(extract)
    };

    // Build canonical reference WKB; bind as bytea for ST_GeomFromEWKB.
    let ref_ewkb = build_reference_ewkb(&geo.geometry, &geo.coordinates)?;
    let ref_geom_expr = validated_geom_from_ewkb(SeaExpr::val(ref_ewkb.to_vec()).into());

    // Build spatial condition using PostGIS helpers
    let spatial_expr = match &geo.georel {
        GeoRel::Within => st_within(geo_extract_expr, ref_geom_expr),
        GeoRel::Contains => st_contains(geo_extract_expr, ref_geom_expr),
        GeoRel::Intersects => st_intersects(geo_extract_expr, ref_geom_expr),
        GeoRel::Disjoint => st_disjoint(geo_extract_expr, ref_geom_expr),
        GeoRel::Equals => st_equals(geo_extract_expr, ref_geom_expr),
        GeoRel::Overlaps => st_overlaps(geo_extract_expr, ref_geom_expr),
        GeoRel::Near { max_distance, min_distance } => {
            // Near uses geography type for accurate distance calculations.
            let geo_col_expr: sea_query::SimpleExpr = if is_default_location {
                SeaExpr::cust("\"location_geo\"")
            } else {
                // JSONB column-side extraction: ST_GeomFromGeoJSON on the stored
                // GeoJSON object is the documented exception to WKB-only binds.
                let jsonpath = format!(
                    "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
                    escape_jsonpath_key(&geo.geoproperty_iri),
                    escape_jsonpath_key(vocab::expanded::properties::VALUE.as_str())
                );
                SeaExpr::cust(format!(
                    "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(jsonb_path_query_first(\"{}\", '{}')::text), 4326))::geography",
                    payload_col, jsonpath
                ))
            };
            let ref_geom_geo_expr: sea_query::SimpleExpr =
                SeaExpr::cust_with_exprs("ST_MakeValid(ST_GeomFromEWKB($1))::geography", [SeaExpr::val(ref_ewkb.to_vec()).into()]);

            let mut parts = Vec::new();
            if let Some(max) = max_distance {
                parts.push(st_dwithin(geo_col_expr.clone(), ref_geom_geo_expr.clone(), *max));
            }
            if let Some(min) = min_distance {
                parts.push(SeaExpr::cust_with_exprs(
                    format!("ST_Distance($1, $2) >= {}", min),
                    [geo_col_expr, ref_geom_geo_expr],
                ));
            }
            if parts.is_empty() {
                return Err(AndromedaError::query_validation("near georel requires maxDistance or minDistance"));
            }
            if parts.len() == 1 {
                parts.remove(0)
            } else {
                let first = parts.remove(0);
                let second = parts.remove(0);
                SeaExpr::cust_with_exprs("($1 AND $2)", [first, second])
            }
        }
    };

    // For default location, add expiration guard to exclude expired location attributes
    if is_default_location {
        let location_iri = vocab::expanded::properties::LOCATION.as_str();
        let expires_at_iri = vocab::expanded::properties::EXPIRES_AT.as_str();
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
            // Distance-based ordering — reference geometry binds as EWKB.
            let ref_ewkb = if let Some(og) = ordering_geometry {
                encode(&og.geometry)?
            } else if let Some(geo) = geo {
                build_reference_ewkb(&geo.geometry, &geo.coordinates)?
            } else {
                return Err(AndromedaError::query_validation(
                    "Distance-based ordering requires either a geo-query or orderFrom parameter".to_string(),
                ));
            };
            let geo_col = build_geo_extract_geography(&term.attr_iri);
            let dist_expr: sea_query::SimpleExpr = SeaExpr::cust_with_exprs(
                format!("ST_Distance({}, ST_MakeValid(ST_GeomFromEWKB($1))::geography)", geo_col),
                [SeaExpr::val(ref_ewkb.to_vec()).into()],
            );
            select.order_by_expr(dist_expr, order);
        } else {
            // Attribute value ordering
            let json_path = format!(
                "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
                escape_jsonpath_key(&term.attr_iri),
                escape_jsonpath_key(vocab::expanded::properties::VALUE.as_str())
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

fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the reference [`Ewkb`] an NGSI-LD `GeoQuery` binds into its spatial
/// predicates, from the query's `geometry`/`coordinates` pair.
fn build_reference_ewkb(geometry: &str, coordinates: &serde_json::Value) -> Result<Ewkb, AndromedaError> {
    let geojson = serde_json::json!({
        "type": geometry,
        "coordinates": coordinates
    });
    let parsed =
        Geometry::from_geojson_value(&geojson).map_err(|error| AndromedaError::query_validation(format!("Invalid geo-query reference geometry: {}", error)))?;
    encode(&parsed)
}

fn build_geo_extract_geography(geoproperty_iri: &str) -> String {
    let is_default_location = geoproperty_iri == vocab::expanded::properties::LOCATION.as_str();
    if is_default_location {
        "\"location_geo\"".to_string()
    } else {
        format!(
            "ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON((jsonb_path_query_first(\"payload\", '$[0].\"{}\"[0].\"{}\"[0].\"@value\"')::text)), 4326))::geography",
            escape_jsonpath_key(geoproperty_iri),
            escape_jsonpath_key(vocab::expanded::properties::VALUE.as_str())
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::{
        domain::temporal::AttributeNameId,
        query::{
            compiled::{
                CompiledEntityQuery,
                CompiledOrderTerm,
                GeoQuery,
                GeoRel,
                IcuCollation,
                IdPattern,
                OrderDirection,
                OrderingGeometry,
                ScopeQuery,
                TypeExpr,
            },
            q::{
                ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
                parser,
            },
            sql::BindValue,
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
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("\"id\" IN"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(IdPattern::new("urn:ngsi-ld:Vehicle:.*").unwrap());
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("\"id\" ~"));
    }

    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.type_expr = Some(TypeExpr::Type("https://example.com/Vehicle".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("\"types\""));
    }

    #[test]
    fn build_with_multiple_types_or() {
        let mut q = empty_query();
        q.type_expr = Some(TypeExpr::Or(vec![TypeExpr::Type("TypeA".to_string()), TypeExpr::Type("TypeB".to_string())]));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("OR"));
    }

    #[test]
    fn build_with_multiple_types_and() {
        let mut q = empty_query();
        q.type_expr = Some(TypeExpr::And(vec![TypeExpr::Type("TypeA".to_string()), TypeExpr::Type("TypeB".to_string())]));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("AND"));
        assert!(built.sql.contains("OR"));
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.pagination.count = true;
        q.type_expr = Some(TypeExpr::Type("Vehicle".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().contains("COUNT(*)"));
    }

    fn temperature_is_known() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([("https://example.com/temperature".to_string(), AttributeNameId::from(12))])
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(parser::parse("\"https://example.com/temperature\"").unwrap());
        let built = build_query(&q, &temperature_is_known()).unwrap();
        // Existence resolves through the temporal catalogue: `payload->0 ? $1`
        // could never use the GIN index on `payload`.
        assert!(built.sql.contains("\"temporal_entity_attributes\" AS \"tea_q\""), "{}", built.sql);
        assert!(!built.sql.contains("\"payload\"->0 ?"), "{}", built.sql);
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        cq.q_expr = Some(Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/temperature"),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        }));
        let built = build_query(&cq, &temperature_is_known()).unwrap();
        assert!(built.sql.contains("EXISTS"), "{}", built.sql);
        assert!(built.sql.contains("\"ac\".\"value_number\" >"), "{}", built.sql);
        assert!(
            !built.sql.contains("jsonb_path_query_first"),
            "value predicates no longer scan the payload: {}",
            built.sql
        );
    }

    #[test]
    fn count_sql_reuses_the_main_query_bind_vector() {
        // The repository binds one vector against both statements, so the
        // placeholder sequence has to be identical up to the pagination
        // parameters the count query omits.
        let mut cq = empty_query();
        cq.pagination.count = true;
        cq.q_expr = Some(Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/temperature"),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        }));
        let built = build_query(&cq, &temperature_is_known()).unwrap();
        let count_sql = built.count_sql.expect("count sql");
        let placeholders = |sql: &str| {
            let mut found: Vec<String> = Vec::new();
            let bytes = sql.as_bytes();
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'$' {
                    let digits: String = sql[index + 1..].chars().take_while(char::is_ascii_digit).collect();
                    if !digits.is_empty() {
                        found.push(digits);
                    }
                }
            }
            found
        };
        let main = placeholders(&built.sql);
        let counted = placeholders(&count_sql);
        assert_eq!(counted, main[..counted.len()], "count query must reuse the leading binds verbatim");
        assert_eq!(counted, vec!["1".to_string(), "2".to_string()], "attribute id then value: {count_sql}");
    }

    #[test]
    fn build_with_limit_offset() {
        let mut q = empty_query();
        q.pagination.limit = 50;
        q.pagination.offset = 100;
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
            geoproperty_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
            geoproperty_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistDesc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("DESC"));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_returns_error() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let result = build_query(&q, &ResolvedAttributeNames::default());
        assert!(result.is_err());
    }

    #[test]
    fn build_with_order_by_dist_with_ordering_geometry() {
        let mut q = empty_query();
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering_geometry = Some(OrderingGeometry::new(geometry));
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("ST_GeomFromEWKB"));
        assert!(
            built.bind_values.iter().any(|b| matches!(b, BindValue::Bytes(_))),
            "reference geometry must be bound as EWKB bytes, not a GeoJSON string"
        );
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
            geoproperty_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
        });
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering_geometry = Some(OrderingGeometry::new(geometry));
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        // Ordering geometry (Berlin) takes precedence over the geo query (Paris);
        // since reference geometries are now bound as EWKB bytes, verify that the
        // bind values carry exactly one raw geometry blob for the distance expression.
        assert!(built.sql.contains("ST_GeomFromEWKB"));
        assert!(built.bind_values.iter().any(|b| matches!(b, BindValue::Bytes(_))));
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
            geoproperty_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
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
            geoproperty_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Within"), "should contain ST_Within spatial predicate");
        assert!(matches!(built.bind_values[1], BindValue::I64(20)), "limit should be I64(20)");
        assert!(matches!(built.bind_values[2], BindValue::I64(0)), "offset should be I64(0)");
    }

    #[test]
    fn georel_variants_bind_reference_geometry_as_ewkb() {
        // Each GeoRel variant must parameterise the reference geometry as
        // EWKB bytes; raw GeoJSON-in-SQL is disallowed in reference position.
        let variants = vec![
            GeoRel::Within,
            GeoRel::Contains,
            GeoRel::Intersects,
            GeoRel::Disjoint,
            GeoRel::Equals,
            GeoRel::Overlaps,
            GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: Some(10.0),
            },
        ];

        for georel in variants {
            let mut q = empty_query();
            q.geo = Some(GeoQuery {
                georel: georel.clone(),
                geometry: "Point".to_string(),
                coordinates: serde_json::json!([1.0, 2.0]),
                geoproperty_iri: vocab::expanded::properties::LOCATION.as_str().to_string(),
            });
            let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
            assert!(built.sql.contains("ST_GeomFromEWKB"), "{georel:?} SQL missing ST_GeomFromEWKB");
            // Default-location path must not re-decode GeoJSON for the reference side.
            assert!(
                !built.sql.contains("ST_GeomFromGeoJSON"),
                "{georel:?} SQL still contains ST_GeomFromGeoJSON: {}",
                built.sql
            );
            assert!(
                built.bind_values.iter().any(|b| matches!(b, BindValue::Bytes(_))),
                "{georel:?} missing EWKB bytes bind"
            );
        }
    }

    #[test]
    fn custom_geoproperty_preserves_jsonb_geojson_extraction() {
        // Custom GeoProperties are stored inside the JSONB payload; the
        // column-side extraction is the documented exception.
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([1.0, 2.0]),
            geoproperty_iri: "https://example.com/customGeo".to_string(),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_GeomFromGeoJSON"), "column-side JSONB extraction must remain");
        assert!(built.sql.contains("ST_GeomFromEWKB"), "reference side must still bind EWKB");
    }

    #[test]
    fn scope_exact_generates_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A/B".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("::lquery"), "SQL should cast to lquery");
    }

    #[test]
    fn scope_hash_generates_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A/#".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("::lquery"));
    }

    #[test]
    fn scope_or_generates_disjunction() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A,/B".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains(" OR "), "SQL should contain OR for comma-separated scopes");
    }

    #[test]
    fn scope_and_generates_conjunction() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A;/B".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND for semicolon-separated scopes");
    }

    #[test]
    fn scope_complex_generates_mixed() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A;/B,/C/#".to_string()));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND");
        assert!(built.sql.contains(" OR "), "SQL should contain OR");
    }
}
