//! Giving every number an attribute holds as JSON the spelling
//! [`JsonNumber`](andromeda_ngsi_ld_number::JsonNumber) renders.
//!
//! A request admitted as a parsed tree has every number respelled before it is
//! decoded. A request decoded straight from its text keeps the spelling its
//! client wrote inside the values it holds as JSON -- a Property value, a
//! `JsonProperty` literal, a list item, a member carried verbatim -- so those,
//! and only those, are respelled after the decode, in place. Geometry
//! ordinates are doubles and typed members carry no number, so neither is
//! touched.

use crate::{
    attribute::{
        common_fields::CommonAttributeFields,
        inline_entity::InlineEntity,
        instance::{Attribute, AttributeValue},
        one_or_many::OneOrMany,
        value::NgsiLdValue,
    },
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::{JsonNumberPathError, canonicalize_numbers};

impl Entity {
    /// Respell every number the entity's attributes hold as JSON.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonNumberPathError`] of a number outside the supported
    /// range, located within the JSON value holding it.
    pub(crate) fn canonicalize_numbers(&mut self) -> Result<(), JsonNumberPathError> {
        [&mut self.location, &mut self.observation_space, &mut self.operation_space]
            .into_iter()
            .flatten()
            .chain(self.attrs.values_mut())
            .try_for_each(AttributeValue::canonicalize_numbers)
    }
}

impl EntityPatch {
    /// Respell every number the patch's attributes hold as JSON.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonNumberPathError`] of a number outside the supported
    /// range, located within the JSON value holding it.
    pub(crate) fn canonicalize_numbers(&mut self) -> Result<(), JsonNumberPathError> {
        [&mut self.location, &mut self.observation_space, &mut self.operation_space]
            .into_iter()
            .flatten()
            .chain(self.attrs.values_mut())
            .try_for_each(AttributeValue::canonicalize_numbers)
    }
}

impl AttributeValue {
    /// Respell every number the member holds as JSON.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonNumberPathError`] of a number outside the supported
    /// range, located within the JSON value holding it.
    pub(crate) fn canonicalize_numbers(&mut self) -> Result<(), JsonNumberPathError> {
        match self {
            AttributeValue::Raw(value) => canonicalize_numbers(value),
            AttributeValue::Single(_) | AttributeValue::Multiple(_) => self.instances_mut().try_for_each(Attribute::canonicalize_numbers),
        }
    }
}

impl Attribute {
    /// Respell every number the instance holds as JSON.
    fn canonicalize_numbers(&mut self) -> Result<(), JsonNumberPathError> {
        match self {
            Attribute::Property(property) => {
                property
                    .previous_value
                    .iter_mut()
                    .chain([&mut property.value])
                    .try_for_each(NgsiLdValue::canonicalize_numbers)?;
            }
            Attribute::ListProperty(property) => {
                property
                    .previous_value_list
                    .iter_mut()
                    .flatten()
                    .chain(property.value_list.iter_mut())
                    .try_for_each(NgsiLdValue::canonicalize_numbers)?;
            }
            Attribute::Relationship(relationship) => relationship.entity.iter_mut().try_for_each(canonicalize_inline)?,
            Attribute::ListRelationship(relationship) => relationship.entity_list.iter_mut().try_for_each(canonicalize_inline)?,
            Attribute::VocabProperty(_) | Attribute::GeoProperty(_) | Attribute::LanguageProperty(_) | Attribute::JsonProperty(_) => {}
        }
        self.common_mut().canonicalize_numbers()
    }
}

impl CommonAttributeFields {
    /// Respell every number the sub-attributes hold as JSON.
    fn canonicalize_numbers(&mut self) -> Result<(), JsonNumberPathError> {
        self.sub_attributes.values_mut().try_for_each(AttributeValue::canonicalize_numbers)
    }
}

impl NgsiLdValue {
    /// Respell every number the value holds, if it is held as JSON.
    fn canonicalize_numbers(&mut self) -> Result<(), JsonNumberPathError> {
        match self {
            NgsiLdValue::Json(value) => canonicalize_numbers(value),
            NgsiLdValue::Geometry(_) => Ok(()),
        }
    }
}

/// Respell every number the entities joined inline hold as JSON.
fn canonicalize_inline(entities: &mut InlineEntity) -> Result<(), JsonNumberPathError> {
    match entities {
        OneOrMany::One(entity) => entity.canonicalize_numbers(),
        OneOrMany::Many(entities) => entities.iter_mut().try_for_each(Entity::canonicalize_numbers),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::Entity;
    use andromeda_ngsi_ld_number::json_value::canonicalize_numbers;
    use serde_json::Value;

    /// Decode `text` as an entity without respelling it, then respell it.
    fn respelled(text: &str) -> Value {
        let mut entity: Entity = serde_json::from_str(text).expect("an entity");
        entity.canonicalize_numbers().expect("supported numbers");
        serde_json::to_value(&entity).expect("an entity renders")
    }

    /// The tree a parsed and respelled `text` gives.
    fn tree(text: &str) -> Value {
        let mut value: Value = serde_json::from_str(text).expect("valid JSON");
        canonicalize_numbers(&mut value).expect("supported numbers");
        value
    }

    #[test]
    fn every_number_held_as_json_takes_the_canonical_spelling() {
        let text = r#"{"id":"urn:ngsi-ld:T:1","type":"T",
            "p":{"type":"Property","value":[1.50,1E+2],"previousValue":1.0,"q":{"type":"Property","value":{"n":2.50}}},
            "j":{"type":"JsonProperty","json":{"a":[1e1]}},
            "l":{"type":"ListProperty","valueList":[3.0,{"x":4.10}]},
            "r":{"type":"Relationship","object":"urn:ngsi-ld:T:2","entity":{"id":"urn:ngsi-ld:T:2","type":"T","n":{"type":"Property","value":5.0}}},
            "raw":{"value":6.0}}"#;
        pretty_assertions::assert_eq!(respelled(text), tree(text));
    }

    /// The sub-attributes of a `GeoProperty` are respelled; its ordinates,
    /// held as doubles, render as doubles do.
    #[test]
    fn the_sub_attributes_of_a_geo_property_take_the_canonical_spelling() {
        let text = r#"{"id":"urn:ngsi-ld:T:1","type":"T",
            "location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1.5,2.5]},"accuracy":{"type":"Property","value":7.0}}}"#;
        pretty_assertions::assert_eq!(respelled(text)["location"]["accuracy"], tree(text)["location"]["accuracy"]);
    }
}
