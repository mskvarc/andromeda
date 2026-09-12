//! Compacting a registration's `information` entries (Table 5.2.9-1).
//!
//! Entity types and attribute names are stored expanded, so every
//! representation of a registration compacts them back through the @context it
//! is rendered under: the discovery response of clause 5.10.2.5, the retrieval
//! of clause 5.10.4.5 and the Context Source Notification of clause 5.11.7
//! alike. One reading, one implementation -- a client that discovers a source
//! and is then notified about it must be handed the same names both times.

use andromeda_jsonld::{
    compaction::{compact_attr_iri, compact_type_iri},
    context_spec::ContextSpec,
    engine::processor::JsonLdEngine,
};
use serde_json::Value;

/// Compact entity types and attribute names in an `information` array.
///
/// A value that is no array carries no entry to compact and is left as it is;
/// the typed deserialisation is what rejects a malformed `information`.
pub async fn compact_information(engine: &JsonLdEngine, information: &mut Value, context: &ContextSpec) {
    let Some(entries) = information.as_array_mut() else {
        return;
    };

    for entry in entries {
        if let Some(entities) = entry.get_mut("entities").and_then(Value::as_array_mut) {
            for entity in entities {
                compact_entity_type(engine, entity, context).await;
            }
        }

        compact_attribute_array(engine, entry, "propertyNames", context).await;
        compact_attribute_array(engine, entry, "relationshipNames", context).await;
    }
}

/// Compact an `EntityInfo` `type` in whichever of its two shapes it was
/// stored (Table 5.2.8-1), keeping that shape.
async fn compact_entity_type(engine: &JsonLdEngine, entity: &mut Value, context: &ContextSpec) {
    let compacted = match entity.get("type") {
        Some(Value::String(type_name)) => Value::String(compact_type_iri(engine, type_name, context).await),
        Some(Value::Array(type_names)) => {
            let mut compacted = Vec::with_capacity(type_names.len());
            for type_name in type_names {
                match type_name {
                    Value::String(type_name) => compacted.push(Value::String(compact_type_iri(engine, type_name, context).await)),
                    other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => compacted.push(other.clone()), // clone: a stored member that is not a name is returned untouched
                }
            }
            Value::Array(compacted)
        }
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) | None => return,
    };
    if let Some(object) = entity.as_object_mut() {
        object.insert("type".to_string(), compacted);
    }
}

/// Compact one of the two attribute-name arrays a `RegistrationInfo` carries.
async fn compact_attribute_array(engine: &JsonLdEngine, information: &mut Value, member: &str, context: &ContextSpec) {
    let Some(names) = information.get(member).and_then(Value::as_array) else {
        return;
    };
    let mut compacted = Vec::with_capacity(names.len());
    for name in names {
        if let Some(raw) = name.as_str() {
            compacted.push(Value::String(compact_attr_iri(engine, raw, context).await));
        }
    }
    if let Some(object) = information.as_object_mut() {
        object.insert(member.to_string(), Value::Array(compacted));
    }
}
