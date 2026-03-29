use crate::{
    attribute::{
        geo_property::GeoProperty,
        json_property::JsonProperty,
        language_property::LanguageProperty,
        list_property::ListProperty,
        list_relationship::ListRelationship,
        property::Property,
        relationship::Relationship,
        vocab_property::VocabProperty,
    },
    error::Result,
    representation::NgsiLdSkipNull,
};
use serde_json::Value as JsonValue;

pub fn serialize_property(prop: &Property, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(serde_json::to_value(&prop.value)?)
}

pub fn serialize_relationship(rel: &Relationship, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(JsonValue::String(rel.object.to_string()))
}

pub fn serialize_geo_property(geo: &GeoProperty, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(serde_json::to_value(&geo.value)?)
}

pub fn serialize_list_relationship(rel: &ListRelationship, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let objects: Vec<JsonValue> = rel.object_list.iter().map(|u| JsonValue::String(u.to_string())).collect();
    Ok(JsonValue::Array(objects))
}

pub fn serialize_language_property(prop: &LanguageProperty, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(serde_json::to_value(&prop.language_map)?)
}

pub fn serialize_vocab_property(prop: &VocabProperty, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(JsonValue::String(prop.has_vocab.clone()))
}

pub fn serialize_list_property(prop: &ListProperty, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(serde_json::to_value(&prop.has_value_list)?)
}

pub fn serialize_json_property(prop: &JsonProperty, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    Ok(serde_json::to_value(&prop.has_json)?)
}
