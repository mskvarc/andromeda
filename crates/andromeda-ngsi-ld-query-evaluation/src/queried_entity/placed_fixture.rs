//! A test entity read whole: the attributes an expanded fixture carries, the
//! points of its `location` and its scopes, so one value drives every trait
//! the entity filters read.

use crate::queried_entity::{
    attribute_reading::QueriedEntity,
    expanded_fixture::{FixtureEntity, FixtureInstance},
    located_entity::LocatedEntity,
    scoped_entity::ScopedEntity,
};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, position::Position};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use serde_json::Value;
use std::borrow::Cow;

/// The expanded name of the NGSI-LD `location` `GeoProperty`, the one
/// attribute the fixture's points are instances of.
pub(crate) const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

/// An entity with its attributes, its `location` points and its scopes.
pub(crate) struct PlacedFixture {
    /// The attributes and types the expanded document carries.
    entity: FixtureEntity,
    /// The instances of `location`, one point each.
    location: Vec<GeometryForm>,
    /// The entity's scopes.
    scopes: Vec<ScopePath>,
}

impl PlacedFixture {
    /// The entity the expanded `document` describes, with a `location` at
    /// `point` (longitude, latitude) when one is given, and in `scopes`.
    pub(crate) fn new(document: &Value, point: Option<[f64; 2]>, scopes: &[&str]) -> PlacedFixture {
        PlacedFixture {
            entity: FixtureEntity::from_expanded(document),
            location: point
                .map(|[longitude, latitude]| GeometryForm::Point(Position::try_new(longitude, latitude, None).expect("a CRS84 position")))
                .into_iter()
                .collect(),
            scopes: scopes.iter().map(|path| path.parse().expect("a scope path")).collect(),
        }
    }
}

impl<'a> QueriedEntity<'a> for &'a PlacedFixture {
    type Instance = &'a FixtureInstance;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a FixtureInstance> {
        self.entity.attribute_instances(attribute)
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        self.entity.types()
    }
}

impl<'a> LocatedEntity<'a> for &'a PlacedFixture {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        let location = attribute.as_str() == LOCATION;
        self.location.iter().filter(move |_| location).map(Cow::Borrowed)
    }
}

impl<'a> ScopedEntity<'a> for &'a PlacedFixture {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        self.scopes.iter()
    }
}
