//! Distance measured on a sphere rather than on the reference spheroid.
//!
//! `ST_Distance` over `geography` operands measures on the spheroid, which is
//! a different measurement from the great-circle distance a sphere gives; the
//! two disagree by roughly a quarter of a percent. A caller has to pick one
//! deliberately, so the spherical measurement is named on its own rather than
//! hiding behind an argument of the spheroidal one.

use sea_query::{Func, Iden, SimpleExpr};

/// The `ST_DistanceSphere(a, b)` function.
///
/// Named as an iden so the function name is rendered by sea-query rather than
/// interpolated, exactly as the spatial predicates are.
#[derive(Iden, Clone, Copy)]
#[iden = "ST_DistanceSphere"]
struct DistanceSphere;

/// `PostGIS`: `ST_DistanceSphere(a, b)` - the shortest distance in metres
/// between two lon/lat geometries, measured on a sphere.
///
/// This is the great-circle (Haversine) distance: the earth is modelled as a
/// sphere whose radius is derived from the operands' spheroid, so the result is
/// the one a Haversine computation over the same coordinates gives. Both
/// operands are `geometry` in a geographic reference system, not `geography`.
#[must_use]
pub fn st_distance_sphere(a: SimpleExpr, b: SimpleExpr) -> SimpleExpr {
    Func::cust(DistanceSphere).args([a, b]).into()
}

#[cfg(test)]
mod tests {
    use crate::spherical_distance::st_distance_sphere;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_spherical_distance_renders_its_own_function_not_st_distance() {
        let sql = Query::select()
            .expr(st_distance_sphere(Expr::col(Alias::new("location")), Expr::val("reference")))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("ST_DistanceSphere("), "{sql}");
        assert!(!sql.contains("::geography"), "the spherical measurement takes geometry operands: {sql}");
    }
}
