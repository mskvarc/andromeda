//! Entities and geoqueries drawn from a seed, for a differential of Query Entities against Subscriptions.
//!
//! The fixed geoquery corpora state what each of their geoqueries selects. A generated case states nothing. It locates Entities at geometries of a small pool, asks geoqueries whose reference geometry comes from the same pool, and expects Query Entities and a Subscription with each geoquery to select the same Entities, whatever those are. The pool holds points, lines and polygons on both sides of the antimeridian, a line and a polygon whose edges cross it, points on a vertex and on an edge of a polygon, and polygons that overlap, so each relation of clause 4.10 holds for some Entities of a case and fails for others. Every seed draws the same case on every run.
//!
//! An Entity has no `location`, one at a geometry, one only in a dataset, or one of each, and may have a second `GeoProperty` `area`. A geoquery is `near` with a maximum or a minimum distance, or one of the other six relations, applied to `location` or, with `geoproperty`, to `area`.

use crate::{
    geometry_round_trip::{line, polygon, rectangle},
    seeded_draw::drawn_from_seed,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        DatasetId,
        Entity,
        EntityId,
        EntityTypeName,
        GeoProperty,
        Input,
        geometry::{Position, Shape},
    },
    query::geoquery::{self, GeoQuery},
};
use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};
use strum::{EnumIter, IntoEnumIterator};

/// The distances, in metres, a `near` geoquery is drawn with.
const DISTANCES: [u32; 3] = [1000, 600_000, 2_000_000];

/// The geometries an Entity is located at and a geoquery refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum PooledGeometry {
    /// The Point (0, 0), a vertex of the square (0, 0) to (10, 10).
    Origin,
    /// The Point (5, 5), inside the squares around the origin and a vertex of the square (5, 5) to (15, 15).
    Inside,
    /// The Point (5, 0), on the south edge of the square (0, 0) to (10, 10), which follows the equator.
    OnEdge,
    /// The Point (180, 0), on the antimeridian.
    Antimeridian,
    /// The Point (175, 5), east of the antimeridian.
    FarEast,
    /// The Point (-175, -5), west of the antimeridian.
    FarWest,
    /// The Point (0, 63), north of the parallel 60 and south of the arc from (-30, 60) to (30, 60), which bows north.
    UnderTheBow,
    /// The `LineString` from (0, 0) to (10, 10).
    Diagonal,
    /// The `LineString` from (170, 0) to (-170, 0), along the equator across the antimeridian.
    AcrossTheAntimeridian,
    /// The `LineString` from (-5, 5) to (5, 5), across the west edge of the square (0, 0) to (10, 10).
    Crossing,
    /// The square (0, 0) to (10, 10).
    Square,
    /// The square (-10, -10) to (10, 10).
    WideSquare,
    /// The square (5, 5) to (15, 15), overlapping the square (0, 0) to (10, 10).
    OverlappingSquare,
    /// The polygon (170, -10), (-170, -10), (-170, 10), (170, 10), 20 degrees wide around the antimeridian.
    AroundTheAntimeridian,
    /// The rectangle (-30, 50) to (30, 60).
    Bowed,
}

impl PooledGeometry {
    /// The geometry as a shape.
    ///
    /// # Errors
    ///
    /// Fails when the shape does not build.
    pub fn shape(self) -> Result<Shape> {
        Ok(match self {
            PooledGeometry::Origin => point(0.0, 0.0)?,
            PooledGeometry::Inside => point(5.0, 5.0)?,
            PooledGeometry::OnEdge => point(5.0, 0.0)?,
            PooledGeometry::Antimeridian => point(180.0, 0.0)?,
            PooledGeometry::FarEast => point(175.0, 5.0)?,
            PooledGeometry::FarWest => point(-175.0, -5.0)?,
            PooledGeometry::UnderTheBow => point(0.0, 63.0)?,
            PooledGeometry::Diagonal => Shape::LineString(line(&[(0.0, 0.0), (10.0, 10.0)])?),
            PooledGeometry::AcrossTheAntimeridian => Shape::LineString(line(&[(170.0, 0.0), (-170.0, 0.0)])?),
            PooledGeometry::Crossing => Shape::LineString(line(&[(-5.0, 5.0), (5.0, 5.0)])?),
            PooledGeometry::Square => rectangle(0.0, 0.0, 10.0, 10.0)?,
            PooledGeometry::WideSquare => rectangle(-10.0, -10.0, 10.0, 10.0)?,
            PooledGeometry::OverlappingSquare => rectangle(5.0, 5.0, 15.0, 15.0)?,
            PooledGeometry::AroundTheAntimeridian => {
                Shape::Polygon(polygon(&[&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]])?)
            }
            PooledGeometry::Bowed => rectangle(-30.0, 50.0, 30.0, 60.0)?,
        })
    }
}

/// The Point at `longitude` and `latitude`.
fn point(longitude: f64, latitude: f64) -> Result<Shape> {
    Ok(Shape::Point(Position::try_new(longitude, latitude)?))
}

/// Where a drawn Entity's `location` is.
#[derive(Debug, Clone, Copy)]
pub enum DrawnLocation {
    /// It has no `location`.
    Nowhere,
    /// Its one `location` instance, the default one.
    Default(PooledGeometry),
    /// Its one `location` instance, in a dataset.
    InDataset(PooledGeometry),
    /// A default `location` instance and a second one in a dataset.
    Both(PooledGeometry, PooledGeometry),
}

/// The `GeoProperty` Attributes of one drawn Entity.
#[derive(Debug, Clone, Copy)]
pub struct DrawnPlace {
    /// Its `location`.
    location: DrawnLocation,
    /// The geometry of its `GeoProperty` `area`, when it has one.
    area: Option<PooledGeometry>,
}

/// The relation a drawn geoquery tests.
#[derive(Debug, Clone, Copy)]
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

/// The `GeoProperty` a drawn geoquery is applied to.
#[derive(Debug, Clone, Copy)]
pub enum DrawnTarget {
    /// `location`, named by no `geoproperty`.
    Location,
    /// `area`, named by `geoproperty`.
    Area,
}

/// One drawn geoquery.
#[derive(Debug, Clone, Copy)]
pub struct DrawnGeoQuery {
    /// Its relation.
    pub relation: DrawnRelation,
    /// Its reference geometry.
    pub geometry: PooledGeometry,
    /// The `GeoProperty` it is applied to.
    pub target: DrawnTarget,
}

impl DrawnGeoQuery {
    /// The geoquery as a client asks it.
    ///
    /// # Errors
    ///
    /// Fails when the reference geometry or the geoquery does not build.
    pub fn geo_query(self) -> Result<GeoQuery> {
        let geometry = self.geometry.shape()?;
        let builder = match self.relation {
            DrawnRelation::NearMax(metres) => geoquery::near_max(metres, geometry),
            DrawnRelation::NearMin(metres) => geoquery::near_min(metres, geometry),
            DrawnRelation::Within => geoquery::within(geometry),
            DrawnRelation::Contains => geoquery::contains(geometry),
            DrawnRelation::Intersects => geoquery::intersects(geometry),
            DrawnRelation::Equals => geoquery::equals(geometry),
            DrawnRelation::Disjoint => geoquery::disjoint(geometry),
            DrawnRelation::Overlaps => geoquery::overlaps(geometry),
        };
        let builder = match self.target {
            DrawnTarget::Location => builder,
            DrawnTarget::Area => builder.geoproperty("area"),
        };
        Ok(builder.build()?)
    }
}

/// A geometry of the pool.
fn pooled() -> BoxedStrategy<PooledGeometry> {
    select(PooledGeometry::iter().collect::<Vec<_>>()).boxed()
}

/// The `GeoProperty` Attributes of an Entity.
fn drawn_place() -> BoxedStrategy<DrawnPlace> {
    let location = prop_oneof![
        1 => Just(DrawnLocation::Nowhere),
        6 => pooled().prop_map(DrawnLocation::Default),
        1 => pooled().prop_map(DrawnLocation::InDataset),
        1 => (pooled(), pooled()).prop_map(|(default, in_dataset)| DrawnLocation::Both(default, in_dataset)),
    ];
    let area = prop_oneof![2 => Just(None), 1 => pooled().prop_map(Some)];
    (location, area).prop_map(|(location, area)| DrawnPlace { location, area }).boxed()
}

/// A geoquery.
fn drawn_geo_query() -> BoxedStrategy<DrawnGeoQuery> {
    let relation = prop_oneof![
        select(&DISTANCES[..]).prop_map(DrawnRelation::NearMax),
        select(&DISTANCES[..]).prop_map(DrawnRelation::NearMin),
        Just(DrawnRelation::Within),
        Just(DrawnRelation::Contains),
        Just(DrawnRelation::Intersects),
        Just(DrawnRelation::Equals),
        Just(DrawnRelation::Disjoint),
        Just(DrawnRelation::Overlaps),
    ];
    let target = prop_oneof![3 => Just(DrawnTarget::Location), 1 => Just(DrawnTarget::Area)];
    (relation, pooled(), target)
        .prop_map(|(relation, geometry, target)| DrawnGeoQuery { relation, geometry, target })
        .boxed()
}

/// A drawn case of located Entities and the geoqueries asked of them.
#[derive(Debug, Clone)]
pub struct GeneratedGeoCase {
    /// The `GeoProperty` Attributes of each Entity.
    places: Vec<DrawnPlace>,
    /// The geoqueries.
    queries: Vec<DrawnGeoQuery>,
}

impl GeneratedGeoCase {
    /// The case `seed` draws, of `entities` Entities and `queries` geoqueries.
    ///
    /// # Errors
    ///
    /// Fails when the generator rejects its draw.
    pub fn drawn(seed: u64, entities: usize, queries: usize) -> Result<GeneratedGeoCase> {
        let (places, queries) = drawn_from_seed(seed, &(vec(drawn_place(), entities), vec(drawn_geo_query(), queries)))?;
        Ok(GeneratedGeoCase { places, queries })
    }

    /// How many Entities the case holds.
    #[must_use]
    pub const fn entity_count(&self) -> usize {
        self.places.len()
    }

    /// The geoqueries of the case.
    #[must_use]
    pub fn queries(&self) -> &[DrawnGeoQuery] {
        &self.queries
    }

    /// The `GeoProperty` Attributes of the Entity at `position`, as the generator drew them, for a failure to name.
    #[must_use]
    pub fn described(&self, position: usize) -> Option<String> {
        self.places.get(position).map(|place| format!("{place:?}"))
    }

    /// The Entities of the case, of `entity_type`, the one at each position identified by `ids` at that position,
    /// a `location` instance in a dataset in `dataset`.
    ///
    /// # Errors
    ///
    /// Fails when there are fewer identifiers than Entities, or a geometry or Entity does not build.
    pub fn documents(&self, entity_type: &EntityTypeName, dataset: &DatasetId, ids: &[EntityId]) -> Result<Vec<Entity<Input>>> {
        if ids.len() < self.places.len() {
            fail!("{} identifiers for {} entities", ids.len(), self.places.len())?;
        }
        let mut documents = Vec::with_capacity(self.places.len());
        for (place, id) in self.places.iter().zip(ids) {
            let builder = Entity::builder().id(id.clone()).entity_type(entity_type.clone());
            let builder = match place.location {
                DrawnLocation::Nowhere => builder,
                DrawnLocation::Default(geometry) => builder.geo_property("location", geometry.shape()?),
                DrawnLocation::InDataset(geometry) => builder.geo_property("location", GeoProperty::builder(geometry.shape()?).dataset_id(dataset.clone())),
                DrawnLocation::Both(default, in_dataset) => builder
                    .geo_property("location", default.shape()?)
                    .geo_property("location", GeoProperty::builder(in_dataset.shape()?).dataset_id(dataset.clone())),
            };
            let builder = match place.area {
                Some(area) => builder.geo_property("area", area.shape()?),
                None => builder,
            };
            documents.push(builder.build()?);
        }
        Ok(documents)
    }
}

#[cfg(test)]
mod tests {
    use crate::geo_generation::{DrawnRelation, GeneratedGeoCase, PooledGeometry};
    use googletest::prelude::*;
    use ngsi_ld::model::{DatasetId, EntityId, EntityTypeName};
    use std::collections::BTreeSet;
    use strum::IntoEnumIterator;

    /// The seeds the suite draws its cases from.
    const SEEDS: u64 = 32;

    /// Names each relation, so that a relation added to the generator is also counted here.
    fn relation(drawn: DrawnRelation) -> &'static str {
        match drawn {
            DrawnRelation::NearMax(_) => "near-max",
            DrawnRelation::NearMin(_) => "near-min",
            DrawnRelation::Within => "within",
            DrawnRelation::Contains => "contains",
            DrawnRelation::Intersects => "intersects",
            DrawnRelation::Equals => "equals",
            DrawnRelation::Disjoint => "disjoint",
            DrawnRelation::Overlaps => "overlaps",
        }
    }

    #[gtest]
    fn every_pooled_geometry_builds() {
        for geometry in PooledGeometry::iter() {
            expect_that!(geometry.shape(), ok(anything()), "{geometry:?}");
        }
    }

    #[gtest]
    fn the_suite_seeds_ask_every_relation_and_every_drawn_query_and_entity_builds() -> Result<()> {
        let entity_type: EntityTypeName = "Drawn".parse()?;
        let dataset: DatasetId = "urn:ngsi-ld:Dataset:drawn".parse()?;
        let ids = (0..12)
            .map(|position| format!("urn:ngsi-ld:Drawn:{position}").parse())
            .collect::<Result<Vec<EntityId>, _>>()?;
        let mut relations = BTreeSet::new();
        for seed in 0..SEEDS {
            let case = GeneratedGeoCase::drawn(seed, 12, 24)?;
            for query in case.queries() {
                relations.insert(relation(query.relation));
                expect_that!(query.geo_query(), ok(anything()), "seed {seed}: {query:?}");
            }
            expect_that!(case.documents(&entity_type, &dataset, &ids), ok(len(eq(12))), "seed {seed}");
        }
        verify_that!(
            relations,
            eq(&BTreeSet::from([
                "contains",
                "disjoint",
                "equals",
                "intersects",
                "near-max",
                "near-min",
                "overlaps",
                "within"
            ]))
        )
    }
}
