//! A written document as the query languages read it.
//!
//! A Temporal Evolution fragment is history the broker records exactly as
//! sent (clauses 5.6.11, 5.6.12); a filter over what a write is about to record
//! reads every instance it carries, several per `datasetId` included, and the
//! scope it sets.

use crate::{
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    geo_value::GeoValue,
    scopes::Scopes,
};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_query::queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity};
use std::borrow::Cow;

impl<'a> QueriedEntity<'a> for &'a EntityDocument {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        self.attributes().get(attribute.as_str()).into_iter().flat_map(AttributeMember::instances)
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        EntityDocument::types(self).iter()
    }
}

impl<'a> LocatedEntity<'a> for &'a EntityDocument {
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        self.attribute_instances(attribute).filter_map(|instance| match instance.value() {
            ValueSlot::Set(WrittenValue::Geometry(geometry)) => Some(Cow::Borrowed(GeoValue::form(geometry))),
            ValueSlot::Set(
                WrittenValue::Value(_)
                | WrittenValue::Object(_)
                | WrittenValue::LanguageMap(_)
                | WrittenValue::Vocab(_)
                | WrittenValue::Json(_)
                | WrittenValue::ValueList(_)
                | WrittenValue::ObjectList(_),
            )
            | ValueSlot::Absent
            | ValueSlot::Remove(_) => None,
        })
    }
}

impl<'a> ScopedEntity<'a> for &'a EntityDocument {
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        self.scope().and_then(Slot::as_set).into_iter().flat_map(Scopes::as_slice)
    }
}
