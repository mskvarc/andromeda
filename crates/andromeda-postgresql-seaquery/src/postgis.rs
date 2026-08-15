//! PostGIS expression constructors for sea-query.
//!
//! sea-query has no native support for PostGIS functions, so the spatial
//! predicates of clause 4.10 are built as custom expressions here rather than
//! inline at each call site.

use crate::{cast::cast, pg_type::PostgresType};
use sea_query::{Expr, Func, Iden, SimpleExpr};

/// The PostGIS functions clause 4.10's spatial predicates are built from.
///
/// Naming each as an iden keeps the function name out of a hand-written
/// template: [`Func::cust`] renders the name and sea-query numbers the operand
/// placeholders, so a value can never be interpolated into the call by mistake.
#[derive(Iden, Clone, Copy)]
enum PostgisFunction {
    /// `ST_Within(a, b)`.
    #[iden = "ST_Within"]
    Within,
    /// `ST_Contains(a, b)`.
    #[iden = "ST_Contains"]
    Contains,
    /// `ST_Intersects(a, b)`.
    #[iden = "ST_Intersects"]
    Intersects,
    /// `ST_Overlaps(a, b)`.
    #[iden = "ST_Overlaps"]
    Overlaps,
    /// `ST_Equals(a, b)`.
    #[iden = "ST_Equals"]
    Equals,
    /// `ST_Disjoint(a, b)`.
    #[iden = "ST_Disjoint"]
    Disjoint,
    /// `ST_DWithin(a, b, distance)`.
    #[iden = "ST_DWithin"]
    Dwithin,
    /// `ST_Distance(a, b)`.
    #[iden = "ST_Distance"]
    Distance,
    /// `ST_GeomFromEWKB(bytes)`.
    #[iden = "ST_GeomFromEWKB"]
    GeomFromEwkb,
    /// `ST_AsEWKB(geom)`.
    #[iden = "ST_AsEWKB"]
    AsEwkb,
    /// `ST_MakeValid(geom)`.
    #[iden = "ST_MakeValid"]
    MakeValid,
    /// `ST_AsGeoJSON(geom)`.
    #[iden = "ST_AsGeoJSON"]
    AsGeoJson,
}

/// Build a spatial function call from its two geometry operands.
fn binary_spatial(function: PostgisFunction, geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Func::cust(function).args([geo_col, ref_geom]).into()
}

/// PostGIS: `ST_Within(geo_col, ref_geom)` - tests if geometry A is within geometry B.
pub fn st_within(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Within, geo_col, ref_geom)
}

/// PostGIS: `ST_DWithin(geo_col, ref_geom, distance)` - tests if geometries are within distance.
///
/// The distance is a request-supplied value, so it binds as a parameter rather
/// than being interpolated into the statement text.
pub fn st_dwithin(geo_col: SimpleExpr, ref_geom: SimpleExpr, distance: f64) -> SimpleExpr {
    Func::cust(PostgisFunction::Dwithin)
        .args([geo_col, ref_geom, Expr::val(distance).into()])
        .into()
}

/// PostGIS: `ST_Contains(geo_col, ref_geom)` - tests if geometry A contains geometry B.
pub fn st_contains(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Contains, geo_col, ref_geom)
}

/// PostGIS: `ST_Distance(geo_col, ref_geom)` - the distance between two geometries,
/// measured on the spheroid when both operands are `geography`.
pub fn st_distance(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Distance, geo_col, ref_geom)
}

/// PostGIS: `ST_Intersects(geo_col, ref_geom)` - tests if geometries intersect.
pub fn st_intersects(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Intersects, geo_col, ref_geom)
}

/// PostGIS: `ST_Overlaps(geo_col, ref_geom)` - tests if geometries overlap.
pub fn st_overlaps(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Overlaps, geo_col, ref_geom)
}

/// PostGIS: `ST_Equals(geo_col, ref_geom)` - tests if geometries are equal.
pub fn st_equals(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Equals, geo_col, ref_geom)
}

/// PostGIS: `ST_Disjoint(geo_col, ref_geom)` - tests if geometries are disjoint.
pub fn st_disjoint(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Disjoint, geo_col, ref_geom)
}

/// PostGIS: `ST_GeomFromEWKB(bytes)` - creates geometry from EWKB (SRID embedded).
fn st_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::GeomFromEwkb).args([ewkb]).into()
}

/// PostGIS: `ST_MakeValid(geom)` - attempts to make an invalid geometry valid.
fn st_make_valid(geom: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::MakeValid).args([geom]).into()
}

/// PostGIS: Builds a validated geometry from EWKB (SRID 4326 already embedded).
///
/// Equivalent to: `ST_MakeValid(ST_GeomFromEWKB($1))`.
pub fn validated_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    st_make_valid(st_geom_from_ewkb(ewkb))
}

/// PostGIS: `ST_AsGeoJSON(geom)::jsonb` - render a stored geometry back as the
/// GeoJSON a response body carries, decoded into JSONB rather than left as text.
pub fn geometry_as_geojson(geometry: SimpleExpr) -> SimpleExpr {
    cast(Func::cust(PostgisFunction::AsGeoJson).args([geometry]).into(), PostgresType::Jsonb)
}

/// PostGIS: `ST_AsEWKB(geom)` - serialise a stored geometry back to EWKB (SRID
/// embedded), the wire form the store decodes into a GeoJSON value in Rust.
pub fn st_as_ewkb(geometry: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::AsEwkb).args([geometry]).into()
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
