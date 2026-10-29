//! The projection of a stored geometry to `geography` the schema indexes.
//!
//! The cast `PostGIS` provides is `STABLE`, which no index expression admits;
//! the migrations wrap it in the `IMMUTABLE` function `geom_to_geog`, and the
//! `geography` index of the hoisted `location` is keyed by that function. A
//! statement names it the same way, so the planner matches the index.

use sea_query::{Func, Iden, SimpleExpr};

/// The function `geom_to_geog(geometry) -> geography`.
#[derive(Iden, Clone, Copy)]
#[iden = "geom_to_geog"]
struct GeomToGeog;

/// `geom_to_geog(geometry)`: `geometry` on the spheroid, as the `geography`
/// index keys it.
#[must_use]
pub fn geography_of(geometry: SimpleExpr) -> SimpleExpr {
    Func::cust(GeomToGeog).arg(geometry).into()
}

#[cfg(test)]
mod tests {
    use crate::geography::geography_of;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_projection_calls_the_function_the_index_is_keyed_by() {
        let sql = Query::select().expr(geography_of(Expr::col("location"))).to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT geom_to_geog("location")"#);
    }
}
