//! The clause 4.5.5 simplified (`keyValues`) rendering of an entity.
//!
//! Two readings of one transformation, kept in one module because they must
//! agree: [`simplify_entity`] walks the typed [`Entity`] and
//! [`simplify_value`](crate::renderer::entity::simplify_value) walks an already-serialised
//! representation, which is what the language-negotiated and inline-join paths
//! carry. The typed reading used to live on `Entity` itself, in the
//! specification model crate — a resource does not render itself.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use chrono::SecondsFormat;
use serde_json::{Map, Value};
use std::str::FromStr;

/// Render an entity in the clause 4.5.5 simplified (`keyValues`) form.
///
/// Walks the typed [`Entity`] directly, which is what lets the simplified path
/// skip a full serialise round-trip. It lived on `Entity` itself until this
/// module existed: a resource is what the specification defines, and how it is
/// rendered is a decision above it.
pub fn simplify_entity(entity: &Entity) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), serde_json::json!(entity.id));
    map.insert("type".to_string(), serde_json::json!(entity.type_));

    if let Some(scope) = &entity.scope {
        map.insert("scope".to_string(), serde_json::json!(scope));
    }

    if let Some(ref expires_at) = entity.expires_at {
        map.insert(
            "expiresAt".to_string(),
            serde_json::json!(expires_at.to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }

    if let Some(ref location) = entity.location {
        map.insert("location".to_string(), simplify_attribute_value(location));
    }

    if let Some(ref observation_space) = entity.observation_space {
        map.insert("observationSpace".to_string(), simplify_attribute_value(observation_space));
    }

    if let Some(ref operation_space) = entity.operation_space {
        map.insert("operationSpace".to_string(), simplify_attribute_value(operation_space));
    }

    if let Some(ref created_at) = entity.created_at {
        map.insert(
            "createdAt".to_string(),
            serde_json::json!(created_at.to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }

    if let Some(ref modified_at) = entity.modified_at {
        map.insert(
            "modifiedAt".to_string(),
            serde_json::json!(modified_at.to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }

    if let Some(ref deleted_at) = entity.deleted_at {
        map.insert(
            "deletedAt".to_string(),
            serde_json::json!(deleted_at.to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }

    for (key, attr) in &entity.attrs {
        map.insert(key.to_string(), simplify_attribute_value(attr));
    }

    Value::Object(map)
}

/// Simplify a single `AttributeValue` to its key-value form.
fn simplify_attribute_value(attr: &AttributeValue) -> Value {
    match attr {
        AttributeValue::Single(a) => a.value(),
        AttributeValue::Multiple(list) => {
            // Dataset map format: {"dataset": {"datasetId1": value1, "@none": value2}}
            let mut dataset_map = Map::new();
            for attr in list {
                let ds_key = attr.dataset_id_string().unwrap_or_else(|| "@none".to_string());
                dataset_map.insert(ds_key, attr.value());
            }
            serde_json::json!({"dataset": dataset_map})
        }
        AttributeValue::Raw(v) => {
            // For language-negotiated or other raw values that still
            // have an NGSI-LD attribute envelope, extract the value.
            // clone: the simplified form is an owned `Value` and the attribute is
            // only borrowed, so every arm below hands back a copy of either the
            // payload member or the whole envelope.
            if let Some(obj) = v.as_object() {
                // The `type` member of a raw envelope is unvalidated JSON, so it
                // is read through `AttributeType`'s own parser; anything it rejects
                // keeps the envelope.
                match obj.get("type").and_then(|kind| kind.as_str()).and_then(|raw| AttributeType::from_str(raw).ok()) {
                    Some(AttributeType::Property | AttributeType::GeoProperty | AttributeType::VocabProperty) => {
                        obj.get("value").cloned().unwrap_or_else(|| v.clone())
                    }
                    Some(AttributeType::Relationship) => obj.get("object").cloned().unwrap_or_else(|| v.clone()),
                    Some(AttributeType::ListProperty) => obj.get("valueList").cloned().unwrap_or_else(|| v.clone()),
                    Some(AttributeType::ListRelationship) => obj.get("objectList").cloned().unwrap_or_else(|| v.clone()),
                    // A language map and a JSON document are already their own
                    // value, and a `type` the parser rejects is not an attribute
                    // envelope at all.
                    Some(AttributeType::LanguageProperty | AttributeType::JsonProperty) | None => v.clone(),
                }
            } else {
                // clone: a raw member that is not an envelope is handed back whole.
                v.clone()
            }
        }
    }
}
