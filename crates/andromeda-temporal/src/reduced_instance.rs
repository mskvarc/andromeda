//! One instance of an Attribute of a merged entity, read on its own by the
//! leaves of a split temporal query's filters (clause 5.7.4.4).
//!
//! Clause 5.7.4.4 keeps an instance of an Attribute that `q` or the geo-query
//! names when both still hold with that Attribute reduced to that instance.
//! The leaves that read the Attribute are read over this view, which holds
//! that one instance and nothing else, under the reading every answered part
//! is read with.

use crate::answered_evolution::read_by_leaves;
use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_query::temporal::window::TemporalWindow;
use andromeda_ngsi_ld_query_evaluation::queried_entity::{attribute_reading::QueriedEntity, located_entity::LocatedEntity, scoped_entity::ScopedEntity};
use std::{borrow::Cow, iter};

/// One instance of one Attribute of an entity, as the leaves reading that
/// Attribute read it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReducedInstance<'a> {
    /// The Attribute the instance belongs to.
    pub(crate) attribute: &'a AttributeIri,
    /// The instance.
    pub(crate) instance: &'a InstanceDocument,
    /// The entity's types, which a hop guard reads.
    pub(crate) types: &'a [EntityTypeName],
    /// The window the leaves read instances inside.
    pub(crate) window: TemporalWindow,
}

impl<'a> QueriedEntity<'a> for &'a ReducedInstance<'a> {
    type Instance = &'a InstanceDocument;

    /// The one instance, when `attribute` is its Attribute and a leaf reads
    /// it over the window.
    fn attribute_instances(self, attribute: &AttributeIri) -> impl Iterator<Item = &'a InstanceDocument> {
        (attribute == self.attribute && read_by_leaves(self.instance, self.window))
            .then_some(self.instance)
            .into_iter()
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        self.types.iter()
    }
}

impl<'a> LocatedEntity<'a> for &'a ReducedInstance<'a> {
    fn geo_property_geometries(self, attribute: &AttributeIri) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
        self.attribute_instances(attribute)
            .filter_map(|instance| instance.geometry().map(Cow::Borrowed))
    }
}

impl<'a> ScopedEntity<'a> for &'a ReducedInstance<'a> {
    /// No Scope, since the restriction an instance meets reads none.
    fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
        iter::empty()
    }
}
