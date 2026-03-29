use crate::{
    attribute::Attributes,
    context::NgsiLdContext,
    error::Result,
    representation::{NgsiLdRepresentation, NgsiLdSerializable, NgsiLdSkipNull, ToRepresentation},
    uri::Uri,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonObject, Value as JsonValue};

/// Entity type — single string or multiple types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum EntityType {
    Single(String),
    Multiple(Vec<String>),
}

/// Entity scope — single string or multiple scopes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Scope {
    Single(String),
    Multiple(Vec<String>),
}

/// A full NGSI-LD Entity (clause 4.5.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entity {
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    pub context: Option<NgsiLdContext>,
    pub id: Uri,
    #[serde(rename = "type")]
    pub type_: EntityType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(flatten)]
    pub attrs: Attributes,
}

impl Entity {
    pub fn new(id: Uri, type_: EntityType) -> Self {
        Self {
            context: None,
            id,
            type_,
            scope: None,
            attrs: Attributes::new(),
        }
    }
}

impl NgsiLdSerializable for Entity {
    fn to_json(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
        let mut map = JsonObject::new();

        if let Some(context) = &self.context {
            map.insert("@context".to_string(), serde_json::to_value(context)?);
        }

        map.insert("id".to_string(), serde_json::to_value(&self.id)?);
        map.insert("type".to_string(), serde_json::to_value(&self.type_)?);

        if let Some(scope) = &self.scope {
            map.insert("scope".to_string(), serde_json::to_value(scope)?);
        }

        for (key, wrapper) in &self.attrs {
            if let Some(val) = wrapper.to_representation(representation, skip_null)? {
                map.insert(key.to_string(), val);
            }
        }

        Ok(JsonValue::Object(map))
    }

    fn to_string_repr(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull, pretty: bool) -> Result<String> {
        let val = self.to_json(representation, skip_null)?;
        if pretty {
            Ok(serde_json::to_string_pretty(&val)?)
        } else {
            Ok(serde_json::to_string(&val)?)
        }
    }
}

impl NgsiLdSerializable for Vec<Entity> {
    fn to_json(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
        let mut vals = Vec::new();
        for entity in self {
            vals.push(entity.to_json(representation, skip_null)?);
        }
        Ok(JsonValue::Array(vals))
    }

    fn to_string_repr(&self, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull, pretty: bool) -> Result<String> {
        let val = self.to_json(representation, skip_null)?;
        if pretty {
            Ok(serde_json::to_string_pretty(&val)?)
        } else {
            Ok(serde_json::to_string(&val)?)
        }
    }
}

/// Partial entity representation for merge/update operations.
///
/// Per the NGSI-LD spec, merge and update operations accept a partial entity
/// where `id` and `type` are optional (since `id` is in the URL path and
/// `type` cannot be changed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uri>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub type_: Option<EntityType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(flatten)]
    pub attrs: Attributes,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::{Attribute, AttributeWrapper, property::Property};
    use ngsi_ld_value::Value;
    use serde_json::json;

    #[test]
    fn entity_deserializes_from_normalized_json() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Eiffel Tower"
            }
        });
        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert_eq!(entity.id, Uri::Urn("urn:ngsi-ld:Building:001".parse().unwrap()));
        assert!(entity.attrs.contains_key(&crate::name::NameBuf::new("name").unwrap()));
    }

    #[test]
    fn entity_deserializes_with_scope() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "scope": "/Madrid/Gardens"
        });
        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert!(entity.scope.is_some());
    }

    #[test]
    fn entity_deserializes_with_multi_type() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": ["Building", "Facility"]
        });
        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert!(matches!(entity.type_, EntityType::Multiple(_)));
    }

    #[test]
    fn entity_deserializes_with_context() {
        let payload = json!({
            "@context": "https://schema.org",
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building"
        });
        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert!(entity.context.is_some());
    }

    #[test]
    fn entity_serializes_to_normalized() {
        let mut entity = Entity::new(Uri::Urn("urn:ngsi-ld:Sensor:001".parse().unwrap()), EntityType::Single("Sensor".to_string()));
        entity.attrs.insert(
            crate::name::NameBuf::new("temperature").unwrap(),
            AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(serde_json::json!(25.5))))),
        );

        let json = entity.to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip).unwrap();
        assert_eq!(json["temperature"]["type"], "Property");
        assert_eq!(json["temperature"]["value"], 25.5);
    }

    #[test]
    fn entity_serializes_to_concise() {
        let mut entity = Entity::new(Uri::Urn("urn:ngsi-ld:Sensor:001".parse().unwrap()), EntityType::Single("Sensor".to_string()));
        entity.attrs.insert(
            crate::name::NameBuf::new("temperature").unwrap(),
            AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(serde_json::json!(25.5))))),
        );

        let json = entity.to_json(NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip).unwrap();
        assert_eq!(json["temperature"], 25.5);
    }

    #[test]
    fn entity_serializes_to_simplified() {
        let mut entity = Entity::new(Uri::Urn("urn:ngsi-ld:Sensor:001".parse().unwrap()), EntityType::Single("Sensor".to_string()));
        entity.attrs.insert(
            crate::name::NameBuf::new("temperature").unwrap(),
            AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(serde_json::json!(25.5))))),
        );

        let json = entity.to_json(NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip).unwrap();
        assert_eq!(json["temperature"], 25.5);
    }

    #[test]
    fn entity_patch_deserializes_without_id() {
        let payload = json!({
            "name": {
                "type": "Property",
                "value": "Updated Name"
            }
        });
        let patch: EntityPatch = serde_json::from_value(payload).unwrap();
        assert!(patch.id.is_none());
        assert!(patch.type_.is_none());
    }

    #[test]
    fn entity_collection_serializes() {
        let entity = Entity::new(Uri::Urn("urn:ngsi-ld:Thing:1".parse().unwrap()), EntityType::Single("Thing".to_string()));
        let entities = vec![entity];
        let json = entities.to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip).unwrap();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[test]
    fn entity_preserves_attribute_order() {
        let mut entity = Entity::new(Uri::Urn("urn:ngsi-ld:Test:order".parse().unwrap()), EntityType::Single("Test".to_string()));
        entity.attrs.insert(
            crate::name::NameBuf::new("z_attr").unwrap(),
            AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(json!("last"))))),
        );
        entity.attrs.insert(
            crate::name::NameBuf::new("a_attr").unwrap(),
            AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(json!("first"))))),
        );

        let json_str = entity.to_string_repr(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip, false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let keys: Vec<&str> = parsed.as_object().unwrap().keys().map(|k| k.as_str()).collect();

        assert_eq!(keys[0], "id");
        assert_eq!(keys[1], "type");
        // Attributes should follow insertion order
        let z_pos = keys.iter().position(|&k| k == "z_attr").unwrap();
        let a_pos = keys.iter().position(|&k| k == "a_attr").unwrap();
        assert!(z_pos < a_pos);
    }
}
