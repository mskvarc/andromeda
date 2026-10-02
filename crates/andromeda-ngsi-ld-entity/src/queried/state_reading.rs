//! An entity state as the query languages read it: its attributes for `q`
//! (clause 4.9), its geometries for a geo-query (clause 4.10) and its scopes
//! for a scope query (clause 4.19).

use crate::{entity_state::EntityState, queried::held_instance::HeldInstance, scopes::Scopes};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_query::queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity};
use std::borrow::Cow;

impl<'a> QueriedEntity<'a> for &'a EntityState {
    type Instance = HeldInstance<'a>;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = HeldInstance<'a>> {
        self.attributes().get(attribute.as_str()).into_iter().flat_map(HeldInstance::every)
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        EntityState::types(self).as_slice().iter()
    }
}

impl<'a> LocatedEntity<'a> for &'a EntityState {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        EntityState::geo_property_geometries(self, attribute.as_str()).map(Cow::Borrowed)
    }
}

impl<'a> ScopedEntity<'a> for &'a EntityState {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        self.scope().into_iter().flat_map(Scopes::as_slice)
    }
}
