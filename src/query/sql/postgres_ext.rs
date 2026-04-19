//! PostgreSQL-specific expression builders for sea-query.
//!
//! Provides custom expression constructors for PostgreSQL features not natively
//! supported by sea-query: PostGIS spatial functions, JSONB operators, ltree
//! matching, and array operations.

use sea_query::{Expr, SimpleExpr};

/// PostGIS: `ST_Within(geo_col, ref_geom)` - tests if geometry A is within geometry B.
pub fn st_within(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Within($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_DWithin(geo_col, ref_geom, distance)` - tests if geometries are within distance.
pub fn st_dwithin(geo_col: SimpleExpr, ref_geom: SimpleExpr, distance: f64) -> SimpleExpr {
    Expr::cust_with_exprs(format!("ST_DWithin($1, $2, {})", distance), [geo_col, ref_geom])
}

/// PostGIS: `ST_Contains(geo_col, ref_geom)` - tests if geometry A contains geometry B.
pub fn st_contains(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Contains($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_Intersects(geo_col, ref_geom)` - tests if geometries intersect.
pub fn st_intersects(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Intersects($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_Overlaps(geo_col, ref_geom)` - tests if geometries overlap.
pub fn st_overlaps(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Overlaps($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_Equals(geo_col, ref_geom)` - tests if geometries are equal.
pub fn st_equals(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Equals($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_Disjoint(geo_col, ref_geom)` - tests if geometries are disjoint.
pub fn st_disjoint(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Disjoint($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_GeomFromGeoJSON(geojson)` - creates geometry from GeoJSON string.
pub fn st_geom_from_geojson(geojson: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_GeomFromGeoJSON($1)", [geojson])
}

/// PostGIS: `ST_GeomFromEWKB(bytes)` - creates geometry from EWKB (SRID embedded).
pub fn st_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_GeomFromEWKB($1)", [ewkb])
}

/// PostGIS: `ST_MakeValid(geom)` - attempts to make an invalid geometry valid.
pub fn st_make_valid(geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_MakeValid($1)", [geom])
}

/// PostGIS: `ST_SetSRID(geom, srid)` - sets the SRID on a geometry.
pub fn st_set_srid(geom: SimpleExpr, srid: i32) -> SimpleExpr {
    Expr::cust_with_exprs(format!("ST_SetSRID($1, {})", srid), [geom])
}

/// PostGIS: Builds a validated geometry from GeoJSON with SRID 4326.
///
/// Equivalent to: `ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($1), 4326))`.
/// Retained for the JSONB column-side extraction case (non-default GeoProperty
/// stored inside `entities.payload`); every other path uses
/// [`validated_geom_from_ewkb`].
pub fn validated_geom_from_geojson(geojson: SimpleExpr) -> SimpleExpr {
    st_make_valid(st_set_srid(st_geom_from_geojson(geojson), 4326))
}

/// PostGIS: Builds a validated geometry from EWKB (SRID 4326 already embedded).
///
/// Equivalent to: `ST_MakeValid(ST_GeomFromEWKB($1))`.
pub fn validated_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    st_make_valid(st_geom_from_ewkb(ewkb))
}

/// JSONB: `col ? key` - tests if JSONB object contains key.
pub fn jsonb_key_exists(col: SimpleExpr, key: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ? $2", [col, key])
}

/// ltree: `col ~ lquery` - tests if ltree matches lquery pattern.
pub fn ltree_matches(col: SimpleExpr, lquery: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2::lquery", [col, lquery])
}

/// Array: `value = ANY(array_col)` - tests if value is in array.
pub fn any_eq(value: SimpleExpr, array_col: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 = ANY($2)", [value, array_col])
}

/// Regex: `col ~ pattern` - tests if column matches POSIX regex.
pub fn regex_match(col: SimpleExpr, pattern: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2", [col, pattern])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn st_within_generates_correct_sql() {
        let expr = st_within(Expr::col(("e", "location")).into(), Expr::val("geojson").into());
        let query = Query::select().expr(expr).to_owned();
        let sql = query.to_string(PostgresQueryBuilder);
        assert!(sql.contains("ST_Within"));
    }

    #[test]
    fn st_dwithin_includes_distance() {
        let expr = st_dwithin(Expr::col(("e", "location")).into(), Expr::val("geojson").into(), 1000.0);
        let query = Query::select().expr(expr).to_owned();
        let sql = query.to_string(PostgresQueryBuilder);
        assert!(sql.contains("ST_DWithin"));
        assert!(sql.contains("1000"));
    }

    #[test]
    fn ltree_matches_uses_lquery_cast() {
        let expr = ltree_matches(Expr::col("scope_paths").into(), Expr::val("A.B.*").into());
        let query = Query::select().expr(expr).to_owned();
        let sql = query.to_string(PostgresQueryBuilder);
        assert!(sql.contains("::lquery"));
    }

    #[test]
    fn any_eq_generates_correct_operator() {
        let expr = any_eq(Expr::val("type_iri").into(), Expr::col("types").into());
        let query = Query::select().expr(expr).to_owned();
        let sql = query.to_string(PostgresQueryBuilder);
        assert!(sql.contains("= ANY"));
    }

    #[test]
    fn validated_geom_from_geojson_nests_functions() {
        let expr = validated_geom_from_geojson(Expr::val("{}").into());
        let query = Query::select().expr(expr).to_owned();
        let sql = query.to_string(PostgresQueryBuilder);
        assert!(sql.contains("ST_MakeValid"));
        assert!(sql.contains("ST_SetSRID"));
        assert!(sql.contains("ST_GeomFromGeoJSON"));
        assert!(sql.contains("4326"));
    }

    #[test]
    fn validated_geom_from_ewkb_generates_st_geomfromewkb() {
        let expr = validated_geom_from_ewkb(Expr::val(Vec::<u8>::new()).into());
        let query = Query::select().expr(expr).to_owned();
        let sql = query.to_string(PostgresQueryBuilder);
        assert!(sql.contains("ST_MakeValid"));
        assert!(sql.contains("ST_GeomFromEWKB"));
        assert!(!sql.contains("ST_GeomFromGeoJSON"));
        assert!(!sql.contains("ST_SetSRID"));
    }
}
