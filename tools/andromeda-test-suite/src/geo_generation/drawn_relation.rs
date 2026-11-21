//! The relations a drawn geoquery tests, and the one geoquery a relation, a pooled reference geometry and the
//! `GeoProperty` it is applied to make, which the generated geoqueries and the generated `q` filters both ask.

use crate::geo_generation::pooled_geometry::PooledGeometry;
use googletest::prelude::*;
use ngsi_ld::query::geoquery::{self, GeoQuery};
use proptest::{
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};

/// The distances, in metres, a `near` geoquery with a maximum distance is drawn with: one that takes in only what
/// touches the reference geometry, and longer ones that take in its region and now and then a region next to it.
const MAXIMUM_DISTANCES: [u32; 3] = [1000, 2_000_000, 3_000_000];

/// The distances, in metres, a `near` geoquery with a minimum distance is drawn with, each leaving out a part of the
/// region of the reference geometry, the region, or the regions around it.
const MINIMUM_DISTANCES: [u32; 3] = [800_000, 2_000_000, 6_000_000];

/// The relation a drawn geoquery tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawnRelation {
    /// `near;maxDistance==` this many metres.
    NearMax(u32),
    /// `near;minDistance==` this many metres.
    NearMin(u32),
    /// `within`.
    Within,
    /// `contains`.
    Contains,
    /// `intersects`.
    Intersects,
    /// `equals`.
    Equals,
    /// `disjoint`.
    Disjoint,
    /// `overlaps`.
    Overlaps,
}

/// Which Entities a relation selects, measured from its reference geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Those at, on or near the reference geometry.
    Including,
    /// Those away from it.
    Excluding,
}

impl DrawnRelation {
    /// Which Entities the relation selects.
    #[must_use]
    pub const fn reach(self) -> Reach {
        match self {
            DrawnRelation::NearMin(_) | DrawnRelation::Disjoint => Reach::Excluding,
            DrawnRelation::NearMax(_)
            | DrawnRelation::Within
            | DrawnRelation::Contains
            | DrawnRelation::Intersects
            | DrawnRelation::Equals
            | DrawnRelation::Overlaps => Reach::Including,
        }
    }
}

/// A relation: the two that select the Entities away from the reference, and `near` with a maximum distance, twice as
/// often as each other one, since what they leave out or take in differs from one region to the next while what the
/// others take in is most often one Entity or none.
pub fn drawn_relation() -> BoxedStrategy<DrawnRelation> {
    prop_oneof![
        2 => select(&MAXIMUM_DISTANCES[..]).prop_map(DrawnRelation::NearMax),
        2 => select(&MINIMUM_DISTANCES[..]).prop_map(DrawnRelation::NearMin),
        1 => Just(DrawnRelation::Within),
        1 => Just(DrawnRelation::Contains),
        1 => Just(DrawnRelation::Intersects),
        1 => Just(DrawnRelation::Equals),
        2 => Just(DrawnRelation::Disjoint),
        1 => Just(DrawnRelation::Overlaps),
    ]
    .boxed()
}

/// The geoquery testing `relation` against `geometry`, applied to the `GeoProperty` `geoproperty` names, or to
/// `location` when it names none.
///
/// # Errors
///
/// Fails when the reference geometry or the geoquery does not build.
pub fn geo_query(relation: DrawnRelation, geometry: PooledGeometry, geoproperty: Option<&str>) -> Result<GeoQuery> {
    let geometry = geometry.shape()?;
    let builder = match relation {
        DrawnRelation::NearMax(metres) => geoquery::near_max(metres, geometry),
        DrawnRelation::NearMin(metres) => geoquery::near_min(metres, geometry),
        DrawnRelation::Within => geoquery::within(geometry),
        DrawnRelation::Contains => geoquery::contains(geometry),
        DrawnRelation::Intersects => geoquery::intersects(geometry),
        DrawnRelation::Equals => geoquery::equals(geometry),
        DrawnRelation::Disjoint => geoquery::disjoint(geometry),
        DrawnRelation::Overlaps => geoquery::overlaps(geometry),
    };
    let builder = match geoproperty {
        Some(name) => builder.geoproperty(name),
        None => builder,
    };
    Ok(builder.build()?)
}
