//! PostGIS expression constructors for sea-query.
//!
//! sea-query has no native support for PostGIS functions, so the spatial
//! predicates of clause 4.10 are built as custom expressions here rather than
//! inline at each call site.

use sea_query::{Expr, SimpleExpr};

/// PostGIS: `ST_Within(geo_col, ref_geom)` - tests if geometry A is within geometry B.
pub fn st_within(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_Within($1, $2)", [geo_col, ref_geom])
}

/// PostGIS: `ST_DWithin(geo_col, ref_geom, distance)` - tests if geometries are within distance.
///
/// The distance is a request-supplied value, so it binds as a parameter rather
/// than being interpolated into the statement text.
pub fn st_dwithin(geo_col: SimpleExpr, ref_geom: SimpleExpr, distance: f64) -> SimpleExpr {
    Expr::cust_with_exprs("ST_DWithin($1, $2, $3)", [geo_col, ref_geom, Expr::val(distance).into()])
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

/// PostGIS: `ST_GeomFromEWKB(bytes)` - creates geometry from EWKB (SRID embedded).
fn st_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_GeomFromEWKB($1)", [ewkb])
}

/// PostGIS: `ST_MakeValid(geom)` - attempts to make an invalid geometry valid.
fn st_make_valid(geom: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("ST_MakeValid($1)", [geom])
}

/// PostGIS: Builds a validated geometry from EWKB (SRID 4326 already embedded).
///
/// Equivalent to: `ST_MakeValid(ST_GeomFromEWKB($1))`.
pub fn validated_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    st_make_valid(st_geom_from_ewkb(ewkb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn st_within_generates_correct_sql() {
        let sql = render(st_within(Expr::col(("e", "location")).into(), Expr::val("geojson").into()));
        assert!(sql.contains("ST_Within"), "{sql}");
    }

    #[test]
    fn st_dwithin_binds_the_distance_rather_than_interpolating_it() {
        let (sql, _values) = Query::select()
            .expr(st_dwithin(Expr::col(("e", "location")).into(), Expr::val("geojson").into(), 1000.0))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("ST_DWithin"), "{sql}");
        // The request-supplied distance must be a bound parameter, never text in
        // the statement, so it cannot appear inline.
        assert!(!sql.contains("1000"), "distance was interpolated, not bound: {sql}");
    }

    #[test]
    fn validated_geom_from_ewkb_generates_st_geomfromewkb() {
        let sql = render(validated_geom_from_ewkb(Expr::val(Vec::<u8>::new()).into()));
        assert!(sql.contains("ST_MakeValid"), "{sql}");
        assert!(sql.contains("ST_GeomFromEWKB"), "{sql}");
        assert!(!sql.contains("ST_GeomFromGeoJSON"), "{sql}");
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
    }
}
