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
    representation::{NgsiLdRepresentation, NgsiLdSkipNull, ToRepresentation},
};
use serde_json::{Map as JsonObject, Value as JsonValue};

pub fn serialize_property(prop: &Property, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("Property".to_string()));
    map.insert("value".to_string(), serde_json::to_value(&prop.value)?);

    if let Some(observed_at) = &prop.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(unit_code) = &prop.unit_code {
        map.insert("unitCode".to_string(), JsonValue::String(unit_code.clone()));
    }
    if let Some(dataset_id) = &prop.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }
    if let Some(instance_id) = &prop.instance_id {
        map.insert("instanceId".to_string(), JsonValue::String(instance_id.clone()));
    }

    for (name, wrapper) in &prop.attributes {
        if let Some(val) = wrapper.to_representation(NgsiLdRepresentation::Normalized, skip_null)? {
            map.insert(name.to_string(), val);
        }
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_relationship(rel: &Relationship, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("Relationship".to_string()));
    map.insert("object".to_string(), JsonValue::String(rel.object.to_string()));

    if let Some(observed_at) = &rel.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(dataset_id) = &rel.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }
    if let Some(instance_id) = &rel.instance_id {
        map.insert("instanceId".to_string(), JsonValue::String(instance_id.clone()));
    }

    for (name, wrapper) in &rel.attributes {
        if let Some(val) = wrapper.to_representation(NgsiLdRepresentation::Normalized, skip_null)? {
            map.insert(name.to_string(), val);
        }
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_geo_property(geo: &GeoProperty, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("GeoProperty".to_string()));
    map.insert("value".to_string(), serde_json::to_value(&geo.value)?);

    if let Some(observed_at) = &geo.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(dataset_id) = &geo.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }
    if let Some(instance_id) = &geo.instance_id {
        map.insert("instanceId".to_string(), JsonValue::String(instance_id.clone()));
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_list_relationship(rel: &ListRelationship, _skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("ListRelationship".to_string()));
    let objects: Vec<JsonValue> = rel.object_list.iter().map(|u| JsonValue::String(u.to_string())).collect();
    map.insert("objectList".to_string(), JsonValue::Array(objects));

    if let Some(obj_type) = &rel.object_type {
        map.insert("objectType".to_string(), JsonValue::String(obj_type.clone()));
    }
    if let Some(observed_at) = &rel.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_language_property(prop: &LanguageProperty, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("LanguageProperty".to_string()));
    map.insert("languageMap".to_string(), serde_json::to_value(&prop.language_map)?);

    if let Some(observed_at) = &prop.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(dataset_id) = &prop.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }

    for (name, wrapper) in &prop.attributes {
        if let Some(val) = wrapper.to_representation(NgsiLdRepresentation::Normalized, skip_null)? {
            map.insert(name.to_string(), val);
        }
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_vocab_property(prop: &VocabProperty, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("VocabProperty".to_string()));
    map.insert("hasVocab".to_string(), JsonValue::String(prop.has_vocab.clone()));

    if let Some(observed_at) = &prop.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(dataset_id) = &prop.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }

    for (name, wrapper) in &prop.attributes {
        if let Some(val) = wrapper.to_representation(NgsiLdRepresentation::Normalized, skip_null)? {
            map.insert(name.to_string(), val);
        }
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_list_property(prop: &ListProperty, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("ListProperty".to_string()));
    map.insert("hasValueList".to_string(), serde_json::to_value(&prop.has_value_list)?);

    if let Some(observed_at) = &prop.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(dataset_id) = &prop.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }

    for (name, wrapper) in &prop.attributes {
        if let Some(val) = wrapper.to_representation(NgsiLdRepresentation::Normalized, skip_null)? {
            map.insert(name.to_string(), val);
        }
    }

    Ok(JsonValue::Object(map))
}

pub fn serialize_json_property(prop: &JsonProperty, skip_null: NgsiLdSkipNull) -> Result<JsonValue> {
    let mut map = JsonObject::new();
    map.insert("type".to_string(), JsonValue::String("JsonProperty".to_string()));
    map.insert("hasJson".to_string(), serde_json::to_value(&prop.has_json)?);

    if let Some(observed_at) = &prop.observed_at {
        map.insert("observedAt".to_string(), serde_json::to_value(observed_at)?);
    }
    if let Some(dataset_id) = &prop.dataset_id {
        map.insert("datasetId".to_string(), JsonValue::String(dataset_id.clone()));
    }

    for (name, wrapper) in &prop.attributes {
        if let Some(val) = wrapper.to_representation(NgsiLdRepresentation::Normalized, skip_null)? {
            map.insert(name.to_string(), val);
        }
    }

    Ok(JsonValue::Object(map))
}
