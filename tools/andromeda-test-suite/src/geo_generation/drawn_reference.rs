//! The reference geometry a drawn geoquery tests its relation against.
//!
//! A geometry of the pool drawn alone meets the geometry of an Entity under `equals`, `within` or `overlaps` only by
//! chance, and a point is disjoint from nearly every Entity. So each relation draws its reference most often from the
//! geometries the Entities of its case hold under the `GeoProperty` it is applied to, and otherwise from the pool:
//! `within` also from its polygons, `contains` from its points, and `overlaps` most often a polygon overlapping one an
//! Entity holds. `disjoint` and a minimum distance draw only a geometry an Entity holds, most often one it holds after
//! both writes, which leaves out the Entities around it in both rounds, where a reference far from every Entity leaves
//! out none and selects the same Entities as every other such reference.

use crate::geo_generation::{
    drawn_relation::DrawnRelation,
    pooled,
    pooled_geometry::{Outline, PooledGeometry},
};
use proptest::{
    prelude::{BoxedStrategy, Strategy},
    sample::select,
    strategy::Union,
};
use strum::IntoEnumIterator;

/// Whether `outline` is a Polygon.
const fn is_polygon(outline: Outline) -> bool {
    match outline {
        Outline::Polygon(_) => true,
        Outline::Point(..) | Outline::LineString(_) => false,
    }
}

/// Whether `outline` is a Point.
const fn is_point(outline: Outline) -> bool {
    match outline {
        Outline::Point(..) => true,
        Outline::LineString(_) | Outline::Polygon(_) => false,
    }
}

/// The geometries of the pool whose outline `keep` lets through.
fn pooled_where(keep: fn(Outline) -> bool) -> Vec<PooledGeometry> {
    PooledGeometry::iter().filter(|geometry| keep(geometry.outline())).collect()
}

/// The reference geometry of a geoquery testing `relation` over Entities that hold the geometries `held` after one
/// write or the other and the geometries `steady` after both, each choice that has a geometry to offer drawn by its
/// weight, and a geometry of the pool when none has.
pub fn drawn_reference(relation: DrawnRelation, held: &[PooledGeometry], steady: &[PooledGeometry]) -> BoxedStrategy<PooledGeometry> {
    let pool: Vec<PooledGeometry> = PooledGeometry::iter().collect();
    let held = held.to_vec();
    let choices: Vec<(u32, Vec<PooledGeometry>)> = match relation {
        DrawnRelation::Disjoint | DrawnRelation::NearMin(_) => vec![(3, steady.to_vec()), (1, held)],
        DrawnRelation::Within => vec![(3, held), (1, pooled_where(is_polygon)), (1, pool)],
        DrawnRelation::Contains => vec![(3, held), (2, pooled_where(is_point)), (1, pool)],
        DrawnRelation::Overlaps => {
            let overlapping = held.iter().filter_map(|geometry| geometry.overlapped()).collect();
            vec![(3, overlapping), (1, held), (1, pool)]
        }
        DrawnRelation::Equals | DrawnRelation::NearMax(_) | DrawnRelation::Intersects => vec![(3, held), (1, pool)],
    };
    let options: Vec<_> = choices
        .into_iter()
        .filter(|(_, geometries)| !geometries.is_empty())
        .map(|(weight, geometries)| (weight, select(geometries)))
        .collect();
    if options.is_empty() {
        return pooled();
    }
    Union::new_weighted(options).boxed()
}
