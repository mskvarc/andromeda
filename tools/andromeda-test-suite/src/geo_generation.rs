//! Entities and geoqueries drawn from a seed, for a differential of Query Entities against Subscriptions.
//!
//! The fixed geoquery corpora state what each of their geoqueries selects. A generated case states nothing. It locates Entities at geometries of a small pool, asks geoqueries whose reference geometry is drawn for its relation from the same pool or from the geometries the Entities of the case hold, and expects Query Entities and a Subscription with each geoquery to select the same Entities, whatever those are. The pool holds points, lines and polygons on both sides of the antimeridian, a line and a polygon whose edges cross it, points on a vertex and on an edge of a polygon, and polygons that overlap, in regions far apart and at several distances within a region, so each relation of clause 4.10 holds for some Entities of a case and fails for others. Every seed draws the same case on every run.
//!
//! An Entity has no `location`, one at a geometry, one only in a dataset, or one of each, and may have a second `GeoProperty` `area`; no two Entities of a case are created at the same `location` geometry while the pool lasts. A second write moves its default `location` instance, and its `area` when it has one, to a geometry of the same region, and adds the Property `moved`, so every Entity raises `attributeCreated`, a default notification trigger (Table 5.2.12-1). The two executions are compared after the Entities are created and again after they are moved. A geoquery is `near` with a maximum or a minimum distance, or one of the other six relations, applied to `location` or, with `geoproperty`, to `area`. No two geoqueries of a case reach the same way from the same region under the same `GeoProperty`, since they would most often select the same Entities.

pub mod drawn_reference;
pub mod drawn_relation;
pub mod pooled_geometry;

use crate::{
    geo_generation::{
        drawn_reference::drawn_reference,
        drawn_relation::{DrawnRelation, Reach, drawn_relation, geo_query},
        pooled_geometry::{PooledGeometry, Region},
    },
    seeded_draw::drawn_from_seed,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{DatasetId, Entity, EntityFragment, EntityId, EntityTypeName, GeoProperty, Input},
    query::geoquery::GeoQuery,
};
use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};
use std::sync::Arc;
use strum::IntoEnumIterator;

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
    /// Where its second write moves it.
    moved: DrawnMove,
}

/// Where the second write of a drawn Entity moves it.
#[derive(Debug, Clone, Copy)]
pub struct DrawnMove {
    /// The geometry of the default `location` instance it writes.
    location: PooledGeometry,
    /// The geometry of the `area` it writes, when it writes one.
    area: Option<PooledGeometry>,
}

/// The `GeoProperty` a drawn geoquery is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawnTarget {
    /// `location`, named by no `geoproperty`.
    Location,
    /// `area`, named by `geoproperty`.
    Area,
}

impl DrawnTarget {
    /// The name `geoproperty` gives, None for `location`, which a geoquery naming none is applied to.
    const fn geoproperty(self) -> Option<&'static str> {
        match self {
            DrawnTarget::Location => None,
            DrawnTarget::Area => Some("area"),
        }
    }
}

/// What two geoqueries of one case share when they most likely select the same Entities. The Entities of a region
/// lie close together and apart from the rest, so a geoquery selects much the same of them, or leaves them out, from
/// any reference geometry of their region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Footprint {
    /// Which Entities the relation selects, measured from the reference geometry.
    reach: Reach,
    /// The region of the reference geometry.
    region: Region,
    /// The `GeoProperty` it is applied to.
    target: DrawnTarget,
}

/// One drawn geoquery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnGeoQuery {
    /// Its relation.
    pub relation: DrawnRelation,
    /// Its reference geometry.
    pub geometry: PooledGeometry,
    /// The `GeoProperty` it is applied to.
    pub target: DrawnTarget,
}

impl DrawnGeoQuery {
    /// The footprint of the geoquery.
    const fn footprint(self) -> Footprint {
        Footprint {
            reach: self.relation.reach(),
            region: self.geometry.region(),
            target: self.target,
        }
    }

    /// The geoquery as a client asks it.
    ///
    /// # Errors
    ///
    /// Fails when the reference geometry or the geoquery does not build.
    pub fn geo_query(self) -> Result<GeoQuery> {
        geo_query(self.relation, self.geometry, self.target.geoproperty())
    }
}

impl DrawnPlace {
    /// Every geometry the Entity holds under `target` after it is created or after it is moved, but the second
    /// `location` instance of an Entity holding two, which a geoquery reaching from it may not tell apart from the
    /// Entities around it, since the other instance can still meet the relation.
    fn geometries(&self, target: DrawnTarget) -> impl Iterator<Item = PooledGeometry> {
        let held = match target {
            DrawnTarget::Location => {
                let created = match self.location {
                    DrawnLocation::Nowhere => None,
                    DrawnLocation::Default(geometry) | DrawnLocation::InDataset(geometry) | DrawnLocation::Both(geometry, _) => Some(geometry),
                };
                [created, Some(self.moved.location)]
            }
            DrawnTarget::Area => [self.area, self.moved.area],
        };
        held.into_iter().flatten()
    }

    /// The geometry the Entity holds under `target` after it is created and still after it is moved, when it holds
    /// one.
    fn steady(self, target: DrawnTarget) -> Option<PooledGeometry> {
        let (created, moved) = match target {
            DrawnTarget::Location => match self.location {
                DrawnLocation::Nowhere => (None, Some(self.moved.location)),
                DrawnLocation::Default(geometry) | DrawnLocation::InDataset(geometry) | DrawnLocation::Both(geometry, _) => {
                    (Some(geometry), Some(self.moved.location))
                }
            },
            DrawnTarget::Area => (self.area, self.moved.area),
        };
        created.filter(|geometry| moved == Some(*geometry))
    }
}

/// A geometry of the pool.
pub fn pooled() -> BoxedStrategy<PooledGeometry> {
    select(PooledGeometry::iter().collect::<Vec<_>>()).boxed()
}

/// A geometry of the pool in the region of `geometry`, `geometry` among them.
fn nearby(geometry: PooledGeometry) -> BoxedStrategy<PooledGeometry> {
    select(PooledGeometry::iter().filter(|other| other.region() == geometry.region()).collect::<Vec<_>>()).boxed()
}

/// The `GeoProperty` Attributes of an Entity whose `location` is created at `at`, when it has one. Its second write
/// moves its `location`, and its `area` when it has one, to a geometry of the same region, so that a region holds the
/// same Entities after both writes and a geoquery reaching from it tells them apart from the rest in both rounds.
fn drawn_place(at: PooledGeometry) -> BoxedStrategy<DrawnPlace> {
    let location = prop_oneof![
        1 => Just(DrawnLocation::Nowhere),
        6 => Just(DrawnLocation::Default(at)),
        1 => Just(DrawnLocation::InDataset(at)),
        1 => pooled().prop_map(move |in_dataset| DrawnLocation::Both(at, in_dataset)),
    ];
    let area = prop_oneof![2 => Just(None), 1 => pooled().prop_map(Some)];
    (location, area, nearby(at))
        .prop_flat_map(|(location, area, moved_location)| {
            let moved_area = match area {
                Some(created) => nearby(created).prop_map(Some).boxed(),
                None => Just(None).boxed(),
            };
            moved_area.prop_map(move |moved_area| DrawnPlace {
                location,
                area,
                moved: DrawnMove {
                    location: moved_location,
                    area: moved_area,
                },
            })
        })
        .boxed()
}

/// The `GeoProperty` Attributes of `entities` Entities. While the pool lasts, no two Entities are created at the same
/// `location` geometry, so that a geoquery tells the Entities its reference geometry relates to apart from their
/// neighbours.
fn drawn_places(entities: usize) -> BoxedStrategy<Vec<DrawnPlace>> {
    Just(PooledGeometry::iter().collect::<Vec<_>>())
        .prop_shuffle()
        .prop_flat_map(move |created| created.into_iter().cycle().take(entities).map(drawn_place).collect::<Vec<_>>())
        .boxed()
}

/// The geometries the Entities of a case hold under one `GeoProperty`.
#[derive(Debug, Clone)]
struct HeldGeometries {
    /// Every geometry an Entity holds after its creation or after its move.
    held: Vec<PooledGeometry>,
    /// Every geometry an Entity holds after both.
    steady: Vec<PooledGeometry>,
}

impl HeldGeometries {
    /// The geometries `places` hold under `target`.
    fn of(places: &[DrawnPlace], target: DrawnTarget) -> HeldGeometries {
        HeldGeometries {
            held: places.iter().flat_map(|place| place.geometries(target)).collect(),
            steady: places.iter().filter_map(|place| place.steady(target)).collect(),
        }
    }
}

/// A geoquery over Entities that hold `location` and `area` under those names, its reference geometry drawn among
/// those of the `GeoProperty` it is applied to.
fn drawn_geo_query(location: Arc<HeldGeometries>, area: Arc<HeldGeometries>) -> BoxedStrategy<DrawnGeoQuery> {
    let target = prop_oneof![3 => Just(DrawnTarget::Location), 1 => Just(DrawnTarget::Area)];
    (drawn_relation(), target)
        .prop_flat_map(move |(relation, target)| {
            let geometries = match target {
                DrawnTarget::Location => &location,
                DrawnTarget::Area => &area,
            };
            drawn_reference(relation, &geometries.held, &geometries.steady).prop_map(move |geometry| DrawnGeoQuery { relation, geometry, target })
        })
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
    /// The case `seed` draws, of `entities` Entities and `queries` geoqueries: of eight times as many drawn, the first
    /// `queries` whose footprint differs from that of every geoquery kept before them.
    ///
    /// # Errors
    ///
    /// Fails when the generator rejects its draw, or fewer than `queries` of the geoqueries drawn differ in footprint.
    pub fn drawn(seed: u64, entities: usize, queries: usize) -> Result<GeneratedGeoCase> {
        let strategy = drawn_places(entities).prop_flat_map(move |places| {
            let location = Arc::new(HeldGeometries::of(&places, DrawnTarget::Location));
            let area = Arc::new(HeldGeometries::of(&places, DrawnTarget::Area));
            (Just(places), vec(drawn_geo_query(location, area), queries * 8))
        });
        let (places, candidates) = drawn_from_seed(seed, &strategy)?;
        let mut distinct: Vec<DrawnGeoQuery> = Vec::with_capacity(queries);
        for candidate in candidates {
            if distinct.len() < queries && distinct.iter().all(|kept| kept.footprint() != candidate.footprint()) {
                distinct.push(candidate);
            }
        }
        if distinct.len() < queries {
            fail!(
                "seed {seed} drew {} geoqueries of distinct footprints, fewer than the {queries} its case asks",
                distinct.len()
            )?;
        }
        Ok(GeneratedGeoCase { places, queries: distinct })
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

    /// The `GeoProperty` Attributes of the Entity at `position` and where it is moved, as the generator drew them, for a
    /// failure to name.
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

    /// The second write of each Entity, at its position, as the fragment Append Entity Attributes takes.
    ///
    /// # Errors
    ///
    /// Fails when a geometry or fragment does not build.
    pub fn changes(&self) -> Result<Vec<EntityFragment>> {
        let mut changes = Vec::with_capacity(self.places.len());
        for place in &self.places {
            let builder = EntityFragment::builder()
                .geo_property("location", place.moved.location.shape()?)
                .property("moved", 1);
            let builder = match place.moved.area {
                Some(area) => builder.geo_property("area", area.shape()?),
                None => builder,
            };
            changes.push(builder.build()?);
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use crate::geo_generation::{
        DrawnLocation,
        DrawnTarget,
        GeneratedGeoCase,
        drawn_relation::DrawnRelation,
        pooled_geometry::{Outline, PooledGeometry},
    };
    use googletest::prelude::*;
    use ngsi_ld::model::{DatasetId, EntityId, EntityTypeName, geometry::Shape};
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
    fn every_pooled_geometry_builds_as_the_type_its_outline_names() -> Result<()> {
        for geometry in PooledGeometry::iter() {
            let built = matches!(
                (geometry.outline(), geometry.shape()?),
                (Outline::Point(..), Shape::Point(_)) | (Outline::LineString(_), Shape::LineString(_)) | (Outline::Polygon(_), Shape::Polygon(_))
            );
            expect_true!(built, "{geometry:?}");
        }
        Ok(())
    }

    #[gtest]
    fn at_least_a_third_of_the_overlaps_geoqueries_refer_to_a_geometry_overlapping_one_an_entity_holds() -> Result<()> {
        let mut overlaps = 0_usize;
        let mut partnered = 0_usize;
        for seed in 0..SEEDS {
            let case = GeneratedGeoCase::drawn(seed, 12, 24)?;
            for query in case.queries().iter().filter(|query| query.relation == DrawnRelation::Overlaps) {
                let mut held = case.places.iter().flat_map(|place| place.geometries(query.target));
                overlaps += 1;
                partnered += usize::from(held.any(|geometry| geometry.overlapped() == Some(query.geometry)));
            }
        }
        verify_that!(partnered * 3, ge(overlaps))
    }

    #[gtest]
    fn no_two_geoqueries_of_a_suite_case_share_a_footprint() -> Result<()> {
        for seed in 0..SEEDS {
            let case = GeneratedGeoCase::drawn(seed, 12, 24)?;
            for (position, query) in case.queries().iter().enumerate() {
                let shared = case.queries().iter().take(position).any(|earlier| earlier.footprint() == query.footprint());
                expect_false!(shared, "seed {seed}: {query:?}");
            }
        }
        Ok(())
    }

    #[gtest]
    fn the_entities_of_a_suite_case_are_created_apart_and_moved_within_their_region() -> Result<()> {
        for seed in 0..SEEDS {
            let case = GeneratedGeoCase::drawn(seed, 12, 24)?;
            let created: Vec<PooledGeometry> = case
                .places
                .iter()
                .filter_map(|place| match place.location {
                    DrawnLocation::Nowhere => None,
                    DrawnLocation::Default(geometry) | DrawnLocation::InDataset(geometry) | DrawnLocation::Both(geometry, _) => Some(geometry),
                })
                .collect();
            for (position, geometry) in created.iter().enumerate() {
                expect_false!(
                    created.iter().take(position).any(|earlier| earlier == geometry),
                    "seed {seed}: {geometry:?} twice"
                );
            }
            for place in &case.places {
                let mut regions = place.geometries(DrawnTarget::Location).map(PooledGeometry::region);
                let first = regions.next();
                expect_true!(regions.all(|region| Some(region) == first), "seed {seed}: {place:?}");
            }
        }
        Ok(())
    }

    #[gtest]
    fn the_suite_seeds_ask_every_relation_and_every_drawn_query_entity_and_move_builds() -> Result<()> {
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
            expect_that!(case.changes(), ok(len(eq(12))), "seed {seed}");
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
