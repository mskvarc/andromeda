//! `PostGIS` expression constructors for sea-query.
//!
//! sea-query has no native support for `PostGIS` functions, so the spatial
//! predicates of clause 4.10 are built as custom expressions here rather than
//! inline at each call site.

use crate::{cast::cast, pg_type::PostgresType};
use sea_query::{BinOper, Expr, ExprTrait, Func, Iden, SimpleExpr};

/// The `PostGIS` functions clause 4.10's spatial predicates are built from.
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
    /// `ST_IsValid(geom)`.
    #[iden = "ST_IsValid"]
    IsValid,
    /// `ST_MakeEnvelope(xmin, ymin, xmax, ymax, srid)`.
    #[iden = "ST_MakeEnvelope"]
    MakeEnvelope,
    /// `ST_Envelope(geom)`.
    #[iden = "ST_Envelope"]
    Envelope,
    /// `ST_Collect(a, b)`, and the aggregate `ST_Collect(geom)`.
    #[iden = "ST_Collect"]
    Collect,
    /// `ST_AsText(geom)`.
    #[iden = "ST_AsText"]
    AsText,
    /// `ST_GeomFromText(wkt, srid)`.
    #[iden = "ST_GeomFromText"]
    GeomFromText,
    /// `ST_MakePoint(x, y)`.
    #[iden = "ST_MakePoint"]
    MakePoint,
    /// `ST_SetSRID(geom, srid)`.
    #[iden = "ST_SetSRID"]
    SetSrid,
    /// `ST_Zmflag(geom)`.
    #[iden = "ST_Zmflag"]
    Zmflag,
}

/// The `PostGIS` bounding-box overlap operator `&&`, true when two geometries'
/// 2D bounding boxes share a point -- the test a geometry `GiST` index answers.
const BOUNDING_BOXES_OVERLAP: &str = "&&";

/// The `PostGIS` bounding-box containment operator `~`, true when the first
/// geometry's 2D bounding box contains the second's.
const BOUNDING_BOX_CONTAINS: &str = "~";

/// The spatial reference system every stored geometry is in (WGS 84).
const WGS84_SRID: i32 = 4326;

/// Build a spatial function call from its two geometry operands.
fn binary_spatial(function: PostgisFunction, geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    Func::cust(function).args([geo_col, ref_geom]).into()
}

/// `PostGIS`: `ST_Within(geo_col, ref_geom)` - tests if geometry A is within geometry B.
#[must_use]
pub fn st_within(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Within, geo_col, ref_geom)
}

/// `PostGIS`: `ST_DWithin(geo_col, ref_geom, distance)` - tests if geometries are within distance.
///
/// The distance is a request-supplied value, so it binds as a parameter rather
/// than being interpolated into the statement text.
#[must_use]
pub fn st_dwithin(geo_col: SimpleExpr, ref_geom: SimpleExpr, distance: f64) -> SimpleExpr {
    Func::cust(PostgisFunction::Dwithin).args([geo_col, ref_geom, Expr::val(distance)]).into()
}

/// `PostGIS`: `ST_Contains(geo_col, ref_geom)` - tests if geometry A contains geometry B.
#[must_use]
pub fn st_contains(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Contains, geo_col, ref_geom)
}

/// `PostGIS`: `ST_Distance(geo_col, ref_geom)` - the distance between two geometries,
/// measured on the spheroid when both operands are `geography`.
#[must_use]
pub fn st_distance(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Distance, geo_col, ref_geom)
}

/// `PostGIS`: `ST_Intersects(geo_col, ref_geom)` - tests if geometries intersect.
#[must_use]
pub fn st_intersects(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Intersects, geo_col, ref_geom)
}

/// `PostGIS`: `ST_Overlaps(geo_col, ref_geom)` - tests if geometries overlap.
#[must_use]
pub fn st_overlaps(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Overlaps, geo_col, ref_geom)
}

/// `PostGIS`: `ST_Equals(geo_col, ref_geom)` - tests if geometries are equal.
#[must_use]
pub fn st_equals(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Equals, geo_col, ref_geom)
}

/// `PostGIS`: `ST_Disjoint(geo_col, ref_geom)` - tests if geometries are disjoint.
#[must_use]
pub fn st_disjoint(geo_col: SimpleExpr, ref_geom: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Disjoint, geo_col, ref_geom)
}

/// `PostGIS`: `ST_MakeEnvelope(min_x, min_y, max_x, max_y, 4326)` - the WGS 84
/// rectangle spanning two longitude/latitude corners.
///
/// The corners are computed from a request, so they bind as parameters.
#[must_use]
pub fn st_make_envelope(min: [f64; 2], max: [f64; 2]) -> SimpleExpr {
    Func::cust(PostgisFunction::MakeEnvelope)
        .args([
            Expr::val(min[0]),
            Expr::val(min[1]),
            Expr::val(max[0]),
            Expr::val(max[1]),
            Expr::val(WGS84_SRID),
        ])
        .into()
}

/// `PostGIS`: `geometry && envelope` - whether the two bounding boxes overlap.
///
/// The one test a geometry `GiST` index answers directly, so it is what a
/// predicate the index cannot otherwise use -- a spheroidal `ST_DWithin` over a
/// cast column -- is prefiltered with.
#[must_use]
pub fn bounding_boxes_overlap(geometry: SimpleExpr, envelope: SimpleExpr) -> SimpleExpr {
    geometry.binary(BinOper::Custom(BOUNDING_BOXES_OVERLAP), envelope)
}

/// `PostGIS`: `geometry::box2d && envelope::box2d` - the same bounding-box
/// overlap as [`bounding_boxes_overlap`], over boxes no geometry index stores.
///
/// A predicate that reads its rows through another index keeps the box test
/// without letting the planner AND a geometry `GiST` into that lookup.
#[must_use]
pub fn unindexed_bounding_boxes_overlap(geometry: SimpleExpr, envelope: SimpleExpr) -> SimpleExpr {
    // One fragment rather than a binary expression, whose operands sea-query
    // would parenthesise: `::` already binds tighter than `&&`.
    Expr::cust_with_exprs(
        format!("$1 {BOUNDING_BOXES_OVERLAP} $2"),
        [cast(geometry, PostgresType::Box2d), cast(envelope, PostgresType::Box2d)],
    )
}

/// `PostGIS`: `geometry ~ other` - whether the bounding box of `geometry`
/// contains the bounding box of `other`.
#[must_use]
pub fn bounding_box_contains(geometry: SimpleExpr, other: SimpleExpr) -> SimpleExpr {
    geometry.binary(BinOper::Custom(BOUNDING_BOX_CONTAINS), other)
}

/// `PostGIS`: `ST_Envelope(geom)` - the bounding box of `geom` as a geometry
/// in its spatial reference system: a polygon, or a point or line where the
/// box has no area.
#[must_use]
pub fn st_envelope(geom: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::Envelope).args([geom]).into()
}

/// `PostGIS`: `ST_Collect(a, b)` - the collection of two geometries; a `NULL`
/// operand contributes nothing, so a `NULL` beside a geometry yields that
/// geometry.
#[must_use]
pub fn st_collect(a: SimpleExpr, b: SimpleExpr) -> SimpleExpr {
    binary_spatial(PostgisFunction::Collect, a, b)
}

/// `PostGIS`: the aggregate `ST_Collect(geom)` - the collection of the
/// geometries of every row of a group, `NULL` ones left out.
#[must_use]
pub fn st_collect_rows(geom: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::Collect).args([geom]).into()
}

/// `PostGIS`: `ST_GeomFromEWKB(bytes)` - creates geometry from EWKB (SRID embedded).
#[must_use]
pub fn st_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::GeomFromEwkb).args([ewkb]).into()
}

/// `PostGIS`: `ST_MakeValid(geom)` - attempts to make an invalid geometry valid.
#[must_use]
pub fn st_make_valid(geom: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::MakeValid).args([geom]).into()
}

/// `PostGIS`: `CASE WHEN ST_IsValid(geom) THEN NULL ELSE ST_MakeValid(geom) END`,
/// the repaired form stored beside a written geometry, NULL when the written
/// one is already valid.
///
/// Clause 4.7.2 admits every IETF RFC 7946 geometry, and RFC 7946 section
/// 3.1.6 sets no simplicity rule on a ring, so a self-intersecting polygon is
/// stored as written; spatial predicates judge this repaired form instead.
#[must_use]
pub fn repaired_when_invalid(geometry: SimpleExpr) -> SimpleExpr {
    // clone: the geometry is tested and then repaired.
    let validity: SimpleExpr = Func::cust(PostgisFunction::IsValid).arg(geometry.clone()).into();
    Expr::case(validity, Expr::null()).finally(st_make_valid(geometry)).into()
}

/// `PostGIS`: `COALESCE(repaired, written)`, the geometry a spatial predicate
/// judges: the repaired form of an invalid written geometry, otherwise the
/// written one.
///
/// Every predicate and every geometry index spells the operand this one way,
/// so the planner matches a predicate to the `GiST` expression index.
#[must_use]
pub fn judged_geometry(repaired: SimpleExpr, written: SimpleExpr) -> SimpleExpr {
    Func::coalesce([repaired, written]).into()
}

/// `PostGIS`: Builds a validated geometry from EWKB (SRID 4326 already embedded).
///
/// Equivalent to: `ST_MakeValid(ST_GeomFromEWKB($1))`.
#[must_use]
pub fn validated_geom_from_ewkb(ewkb: SimpleExpr) -> SimpleExpr {
    st_make_valid(st_geom_from_ewkb(ewkb))
}

/// `PostGIS`: `ST_AsEWKB(geom)` - serialise a stored geometry back to EWKB (SRID
/// embedded), the wire form the store decodes into a `GeoJSON` value in Rust.
#[must_use]
pub fn st_as_ewkb(geometry: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::AsEwkb).args([geometry]).into()
}

/// `PostGIS`: `ST_AsText(geometry)` - the well-known text of a geometry.
#[must_use]
pub fn st_as_text(geometry: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::AsText).args([geometry]).into()
}

/// `PostGIS`: `ST_GeomFromText(wkt, 4326)` - a WGS 84 geometry from its
/// well-known text.
#[must_use]
pub fn st_geom_from_text_wgs84(wkt: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::GeomFromText).args([wkt, Expr::val(WGS84_SRID)]).into()
}

/// `PostGIS`: `ST_SetSRID(ST_MakePoint(x, y), 4326)` - the WGS 84 point at
/// longitude `x`, latitude `y`.
#[must_use]
pub fn st_point_wgs84(x: SimpleExpr, y: SimpleExpr) -> SimpleExpr {
    let point = Func::cust(PostgisFunction::MakePoint).args([x, y]);
    Func::cust(PostgisFunction::SetSrid).args([point.into(), Expr::val(WGS84_SRID)]).into()
}

/// `PostGIS`: `ST_Zmflag(geometry)` - 0 for a 2D geometry, 1 for one with a
/// measure, 2 for one with a height, 3 for both.
#[must_use]
pub fn st_zmflag(geometry: SimpleExpr) -> SimpleExpr {
    Func::cust(PostgisFunction::Zmflag).args([geometry]).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_wgs84_point_names_its_srid_around_the_made_point() {
        pretty_assertions::assert_eq!(
            render(st_point_wgs84(Expr::val(13.5), Expr::val(52.5))),
            "SELECT ST_SetSRID(ST_MakePoint(13.5, 52.5), 4326)"
        );
    }

    #[test]
    fn the_text_and_flag_functions_take_the_geometry_positionally() {
        pretty_assertions::assert_eq!(render(st_as_text(Expr::col("location"))), r#"SELECT ST_AsText("location")"#);
        pretty_assertions::assert_eq!(render(st_zmflag(Expr::col("location"))), r#"SELECT ST_Zmflag("location")"#);
        pretty_assertions::assert_eq!(
            render(st_geom_from_text_wgs84(Expr::val("POINT(1 2)"))),
            "SELECT ST_GeomFromText('POINT(1 2)', 4326)"
        );
    }

    #[test]
    fn an_envelope_binds_its_corners_and_names_the_wgs84_srid() {
        let (sql, values) = Query::select()
            .expr(bounding_boxes_overlap(
                Expr::col(("c", "value_geometry")),
                st_make_envelope([1.0, 2.0], [3.0, 4.0]),
            ))
            .to_owned()
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT "c"."value_geometry" && ST_MakeEnvelope($1, $2, $3, $4, $5)"#);
        pretty_assertions::assert_eq!(values.0.len(), 5);
    }

    #[test]
    fn an_unindexed_overlap_compares_both_operands_as_box2d() {
        let (sql, values) = Query::select()
            .expr(unindexed_bounding_boxes_overlap(
                Expr::col(("c", "value_geometry")),
                st_make_envelope([1.0, 2.0], [3.0, 4.0]),
            ))
            .to_owned()
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT "c"."value_geometry"::box2d && ST_MakeEnvelope($1, $2, $3, $4, $5)::box2d"#);
        pretty_assertions::assert_eq!(values.0.len(), 5);
    }

    #[test]
    fn an_extent_grows_by_the_envelope_of_its_collection_with_a_geometry() {
        let sql = render(st_envelope(st_collect(
            Expr::col(("s", "value_extent")),
            st_envelope(st_collect_rows(Expr::col(("e", "value_geometry")))),
        )));
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT ST_Envelope(ST_Collect("s"."value_extent", ST_Envelope(ST_Collect("e"."value_geometry"))))"#
        );
    }

    #[test]
    fn a_bounding_box_containment_renders_the_tilde_operator() {
        let sql = render(bounding_box_contains(Expr::col(("s", "value_extent")), Expr::col(("g", "extent"))));
        pretty_assertions::assert_eq!(sql, r#"SELECT "s"."value_extent" ~ "g"."extent""#);
    }

    #[test]
    fn st_within_generates_correct_sql() {
        let sql = render(st_within(Expr::col(("e", "location")), Expr::val("geojson")));
        assert!(sql.contains("ST_Within"), "{sql}");
    }

    #[test]
    fn st_dwithin_binds_the_distance_rather_than_interpolating_it() {
        let (sql, _values) = Query::select()
            .expr(st_dwithin(Expr::col(("e", "location")), Expr::val("geojson"), 1000.0))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("ST_DWithin"), "{sql}");
        // The request-supplied distance must be a bound parameter, never text in
        // the statement, so it cannot appear inline.
        assert!(!sql.contains("1000"), "distance was interpolated, not bound: {sql}");
    }

    #[test]
    fn repaired_when_invalid_is_null_for_a_valid_geometry_and_made_valid_otherwise() {
        pretty_assertions::assert_eq!(
            render(repaired_when_invalid(Expr::col("x"))),
            r#"SELECT (CASE WHEN (ST_IsValid("x")) THEN NULL ELSE ST_MakeValid("x") END)"#
        );
    }

    #[test]
    fn the_judged_geometry_prefers_the_repaired_form() {
        pretty_assertions::assert_eq!(
            render(judged_geometry(Expr::col(("a", "value_geometry_valid")), Expr::col(("a", "value_geometry")))),
            r#"SELECT COALESCE("a"."value_geometry_valid", "a"."value_geometry")"#
        );
    }

    #[test]
    fn validated_geom_from_ewkb_generates_st_geomfromewkb() {
        let sql = render(validated_geom_from_ewkb(Expr::val(Vec::<u8>::new())));
        assert!(sql.contains("ST_MakeValid"), "{sql}");
        assert!(sql.contains("ST_GeomFromEWKB"), "{sql}");
        assert!(!sql.contains("ST_GeomFromGeoJSON"), "{sql}");
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
    }
}
