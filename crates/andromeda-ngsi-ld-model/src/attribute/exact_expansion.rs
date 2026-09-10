//! `serde_json::Value`-native expansion for arbitrary-precision NGSI-LD values.

use crate::{
    attribute::{
        common_fields::CommonAttributeFields,
        geo_property::GeoProperty,
        instance::{Attribute, AttributeValue},
        json_property::JsonProperty,
        language_property::LanguageProperty,
        list_property::ListProperty,
        list_relationship::ListRelationship,
        one_or_many::OneOrMany,
        property::Property,
        relationship::Relationship,
        value::NgsiLdValue,
        vocab_property::VocabProperty,
    },
    entity::Entity,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use jsonld::Expandable;
use jsonld_expandable_core::ToJsonValue;
use serde_json::{Map, Value, json};

/// Expand a model value directly into `serde_json::Value` without narrowing
/// arbitrary-precision JSON numbers.
pub trait ExpandToJson {
    /// Produce the exact expanded JSON-LD representation.
    fn expand_to_json(&self) -> Value;
}

impl ExpandToJson for Entity {
    fn expand_to_json(&self) -> Value {
        let mut expanded = Expandable::expand::<Value>(self);
        let Some(object) = expanded.as_object_mut() else {
            return expanded;
        };
        overwrite_optional_attribute(object, expanded_properties::LOCATION.as_str(), self.location.as_ref());
        overwrite_optional_attribute(object, expanded_properties::OBSERVATION_SPACE.as_str(), self.observation_space.as_ref());
        overwrite_optional_attribute(object, expanded_properties::OPERATION_SPACE.as_str(), self.operation_space.as_ref());
        self.attrs.iter().for_each(|(name, value)| {
            object.insert(name.as_str().to_string(), value.expand_to_json());
        });
        expanded
    }
}

impl ExpandToJson for AttributeValue {
    fn expand_to_json(&self) -> Value {
        match self {
            AttributeValue::Single(attribute) => Value::Array(vec![attribute.expand_to_json()]),
            AttributeValue::Multiple(attributes) => Value::Array(attributes.iter().map(ExpandToJson::expand_to_json).collect()),
            AttributeValue::Raw(value) => json!([{"@value": value}]),
        }
    }
}

impl ExpandToJson for Attribute {
    fn expand_to_json(&self) -> Value {
        match self {
            Attribute::Property(value) => value.expand_to_json(),
            Attribute::Relationship(value) => value.expand_to_json(),
            Attribute::VocabProperty(value) => value.expand_to_json(),
            Attribute::GeoProperty(value) => value.expand_to_json(),
            Attribute::LanguageProperty(value) => value.expand_to_json(),
            Attribute::JsonProperty(value) => value.expand_to_json(),
            Attribute::ListProperty(value) => value.expand_to_json(),
            Attribute::ListRelationship(value) => value.expand_to_json(),
        }
    }
}

impl ExpandToJson for Property {
    fn expand_to_json(&self) -> Value {
        let mut expanded = scaffold_without_sub_attributes(self);
        let Some(object) = expanded.as_object_mut() else {
            return expanded;
        };
        object.insert(expanded_properties::VALUE.as_str().to_string(), value_nodes(&self.value));
        if let Some(previous) = &self.previous_value {
            object.insert(expanded_properties::PREVIOUS_VALUE.as_str().to_string(), value_nodes(previous));
        }
        overwrite_sub_attributes(object, &self.common);
        expanded
    }
}

impl ExpandToJson for JsonProperty {
    fn expand_to_json(&self) -> Value {
        let mut expanded = scaffold_without_sub_attributes(self);
        let Some(object) = expanded.as_object_mut() else {
            return expanded;
        };
        object.insert(expanded_properties::JSON.as_str().to_string(), json_nodes(&self.json));
        if let Some(previous) = &self.previous_json {
            object.insert(expanded_properties::PREVIOUS_JSON.as_str().to_string(), json_nodes(previous));
        }
        overwrite_sub_attributes(object, &self.common);
        expanded
    }
}

impl ExpandToJson for ListProperty {
    fn expand_to_json(&self) -> Value {
        let mut expanded = scaffold_without_sub_attributes(self);
        let Some(object) = expanded.as_object_mut() else {
            return expanded;
        };
        object.insert(expanded_properties::VALUE_LIST.as_str().to_string(), list_nodes(&self.value_list));
        if let Some(previous) = &self.previous_value_list {
            object.insert(expanded_properties::PREVIOUS_VALUE_LIST.as_str().to_string(), list_nodes(previous));
        }
        overwrite_sub_attributes(object, &self.common);
        expanded
    }
}

impl ExpandToJson for Relationship {
    fn expand_to_json(&self) -> Value {
        let mut expanded = scaffold_without_sub_attributes(self);
        let Some(object) = expanded.as_object_mut() else {
            return expanded;
        };
        if let Some(entity) = &self.entity {
            object.insert(expanded_properties::ENTITY.as_str().to_string(), expand_inline_entities(entity));
        }
        overwrite_sub_attributes(object, &self.common);
        expanded
    }
}

impl ExpandToJson for ListRelationship {
    fn expand_to_json(&self) -> Value {
        let mut expanded = scaffold_without_sub_attributes(self);
        let Some(object) = expanded.as_object_mut() else {
            return expanded;
        };
        if let Some(entities) = &self.entity_list {
            object.insert(expanded_properties::ENTITY_LIST.as_str().to_string(), expand_inline_entities(entities));
        }
        overwrite_sub_attributes(object, &self.common);
        expanded
    }
}

/// Implement exact expansion for families whose value-bearing members are all contained in their recursively expanded common fields.
macro_rules! impl_scaffold_expansion {
    ($($family:ty),+ $(,)?) => {
        $(
            impl ExpandToJson for $family {
                fn expand_to_json(&self) -> Value {
                    let mut expanded = scaffold_without_sub_attributes(self);
                    if let Some(object) = expanded.as_object_mut() {
                        overwrite_sub_attributes(object, &self.common);
                    }
                    expanded
                }
            }
        )+
    };
}

impl_scaffold_expansion!(GeoProperty, LanguageProperty, VocabProperty);

/// Use the derive-generated structure after removing the only recursively
/// numeric field. The clone is required because expansion borrows the family
/// while the caller's model must remain unchanged.
fn scaffold_without_sub_attributes<T>(value: &T) -> Value
where
    T: Clone + Expandable + HasCommonFields,
{
    let mut scaffold = value.clone();
    scaffold.common_mut().sub_attributes.clear();
    Expandable::expand::<Value>(&scaffold)
}

/// Access the common fields on an attribute family scaffold.
trait HasCommonFields {
    /// Mutably borrow the family common fields.
    fn common_mut(&mut self) -> &mut CommonAttributeFields;
}

/// Implement mutable common-field access for each attribute-family scaffold.
macro_rules! impl_common_fields {
    ($($family:ty),+ $(,)?) => {
        $(
            impl HasCommonFields for $family {
                fn common_mut(&mut self) -> &mut CommonAttributeFields {
                    &mut self.common
                }
            }
        )+
    };
}

impl_common_fields!(
    Property,
    Relationship,
    VocabProperty,
    GeoProperty,
    LanguageProperty,
    JsonProperty,
    ListProperty,
    ListRelationship
);

/// Replace every sub-attribute scaffold with its exact recursive expansion.
fn overwrite_sub_attributes(object: &mut Map<String, Value>, common: &CommonAttributeFields) {
    common.sub_attributes.iter().for_each(|(name, value)| {
        object.insert(name.as_str().to_string(), value.expand_to_json());
    });
}

/// Replace one optional entity-level attribute when it is present.
fn overwrite_optional_attribute(object: &mut Map<String, Value>, name: &str, value: Option<&AttributeValue>) {
    if let Some(value) = value {
        object.insert(name.to_string(), value.expand_to_json());
    }
}

/// Expanded `@value` node for one exact Property value.
fn value_nodes(value: &NgsiLdValue) -> Value {
    json!([{"@value": ngsi_ld_value(value)}])
}

/// Expanded `@json` node retaining every nested native number.
fn json_nodes(value: &Value) -> Value {
    json!([{"@value": value, "@type": "@json"}])
}

/// Expanded list container retaining every item as its native JSON value.
fn list_nodes(values: &[NgsiLdValue]) -> Value {
    json!([{"@list": values.iter().map(ngsi_ld_value).collect::<Vec<_>>() }])
}

/// Convert the NGSI-LD value family to the concrete exact JSON backend.
fn ngsi_ld_value(value: &NgsiLdValue) -> Value {
    match value {
        NgsiLdValue::Json(value) => value.clone(),
        NgsiLdValue::Geometry(value) => ToJsonValue::<Value>::to_json_value(value),
    }
}

/// Expand one or many inline entities without the generic numeric bridge.
fn expand_inline_entities(entities: &OneOrMany<Entity>) -> Value {
    match entities {
        OneOrMany::One(entity) => entity.expand_to_json(),
        OneOrMany::Many(entities) => Value::Array(entities.iter().map(ExpandToJson::expand_to_json).collect()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute::exact_expansion::ExpandToJson, entity::Entity};
    use serde_json::Value;

    #[test]
    fn exact_expansion_preserves_property_compound_list_and_json_numbers() {
        let entity: Entity = serde_json::from_str(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","scalar":{"type":"Property","value":0.100000000000000005,"nested":{"type":"Property","value":0.100000000000000005}},"list":{"type":"ListProperty","valueList":[0.100000000000000005]},"json":{"type":"JsonProperty","json":{"number":0.100000000000000005}}}"#,
        )
        .expect("an exact entity");
        let expanded = entity.expand_to_json();
        let output = serde_json::to_string(&expanded).expect("expanded JSON");
        pretty_assertions::assert_eq!(output.matches("0.100000000000000005").count(), 4);
        assert!(!output.contains("$serde_json::private::Number"), "got {output}");
    }

    #[test]
    fn exact_expansion_returns_native_numbers_instead_of_strings() {
        let entity: Entity =
            serde_json::from_str(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}}"#)
                .expect("an exact entity");
        let expanded = entity.expand_to_json();
        let value = &expanded["https://example.org/unused"];
        pretty_assertions::assert_eq!(value, &Value::Null);
        let output = serde_json::to_string(&expanded).expect("expanded JSON");
        assert!(output.contains("0.100000000000000005"), "got {output}");
    }
}
