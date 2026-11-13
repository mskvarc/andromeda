//! The geospatial members an entity holds only as a `GeoProperty`.
//!
//! Table 5.2.4-1 types `location`, `observationSpace` and `operationSpace` as
//! `GeoProperty`, and clause 5.2.20 makes the Temporal Evolution of an Entity
//! the same datatype, so clause 5.5.4 refuses a write holding one of them as
//! any other attribute family. The members are told apart by their expanded
//! IRI, so the rule reaches them however a request spells them: as the core
//! term, as a compact IRI, as a term of the user `@context` or as the IRI
//! itself.
//!
//! An instance is held to the family it declares, or else to the one its value
//! member implies. One that does neither keeps the family of the instance it
//! updates (clauses 5.5.8 and 5.6.4), and one whose value is the NGSI-LD Null
//! deletes a value rather than writing one, so neither is judged here. Table
//! 5.2.4-1 types the members of the entity, not those of an instance, so a
//! sub-attribute is not held to the rule.

use crate::{
    document::{EntityDocument, instance_document::InstanceDocument, written_value::ValueSlot},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::Iri;

/// The members Table 5.2.4-1 types as `GeoProperty`.
const GEO_MEMBERS: [Iri<&'static str>; 3] = [properties::LOCATION, properties::OBSERVATION_SPACE, properties::OPERATION_SPACE];

impl EntityDocument {
    /// Hold every instance of the geospatial members of the document to the
    /// `GeoProperty` family.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::NotAGeoProperty`] for the first instance
    /// written as another family.
    pub fn admit_geo_members(&self) -> Result<(), EntityRefusal> {
        self.attributes()
            .iter()
            .filter(|(name, _member)| is_geo_member(name))
            .try_for_each(|(name, member)| member.instances().iter().try_for_each(|instance| admit_geo_instance(name, instance)))
    }
}

impl InstanceDocument {
    /// Hold the instance written for `attribute` to the `GeoProperty` family
    /// when `attribute` is one of the geospatial members.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::NotAGeoProperty`] when it is written as
    /// another family.
    pub fn admit_geo_member(&self, attribute: &AttributeName) -> Result<(), EntityRefusal> {
        if is_geo_member(attribute) {
            admit_geo_instance(attribute, self)
        } else {
            Ok(())
        }
    }
}

/// Whether the expanded name `name` is one of the geospatial members.
fn is_geo_member(name: &AttributeName) -> bool {
    GEO_MEMBERS.iter().any(|member| member.as_str() == name.as_str())
}

/// Hold one instance of the geospatial member `name` to `GeoProperty`.
fn admit_geo_instance(name: &AttributeName, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    let family = match instance.value() {
        ValueSlot::Remove(_) => return Ok(()),
        ValueSlot::Set(_) | ValueSlot::Absent => instance.implied_family(),
    };
    match family {
        None | Some(AttributeType::GeoProperty) => Ok(()),
        Some(
            family @ (AttributeType::Property
            | AttributeType::Relationship
            | AttributeType::LanguageProperty
            | AttributeType::VocabProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty
            | AttributeType::ListRelationship),
        ) => Err(EntityRefusal::NotAGeoProperty {
            attribute: name.clone(), // clone: the refusal outlives the borrowed name, so it owns a copy
            family,
        }),
    }
}
