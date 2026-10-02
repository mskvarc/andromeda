//! The processor's compaction, as the read path has always performed it.
//!
//! This is the reading the walk is held to: the stored document with its
//! attribute-instance system times stripped when the request did not ask for
//! `sysAttrs`, compacted by the processor, reshaped to the compact model's
//! `id` and `type` and decoded, its expired instances dropped.

use andromeda_jsonld::engine::{context_compactor::ContextCompactor, expanded_input::ExpandedInput, options::CompactOptions};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The compact entity the processor gives for a stored document, or the
/// rendering of its failure.
pub(crate) async fn processor_entity(
    compactor: &ContextCompactor<'_>,
    stored: &Value,
    id_override: Option<&str>,
    sys_attrs: SysAttrsInclusion,
    now: DateTime<Utc>,
) -> Result<Entity, String> {
    let expanded = match sys_attrs {
        SysAttrsInclusion::Include => stored.clone(), // clone: the processor consumes a copy, the corpus keeps the stored document
        SysAttrsInclusion::Strip => {
            let mut state = EntityState::from_slice(&serde_json::to_vec(stored).expect("the stored document renders")).expect("the stored document is a state");
            state.strip_attribute_instants();
            serde_json::to_value(&state).expect("the stripped state renders")
        }
    };
    let compacted = compactor
        .compact(ExpandedInput::Canonical(expanded), CompactOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    let mut object = match compacted {
        Value::Array(mut items) if !items.is_empty() => match items.swap_remove(0) {
            Value::Object(object) => Some(object),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
        },
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
    }
    .ok_or_else(|| "the compacted entity is not a JSON object".to_owned())?;
    if let Some(types) = object.remove("@type") {
        object.insert("type".to_owned(), types);
    }
    let id = id_override
        .map(str::to_owned)
        .or_else(|| object.get("id").and_then(Value::as_str).map(str::to_owned))
        .or_else(|| object.get("@id").and_then(Value::as_str).map(str::to_owned))
        .ok_or_else(|| "the compacted entity carries no id".to_owned())?;
    object.remove("@id");
    object.insert("id".to_owned(), Value::String(id));
    let mut entity: Entity = deserialize_exact(&Value::Object(object)).map_err(|error| error.to_string())?;
    entity.remove_expired_attributes(now);
    Ok(entity)
}

/// The compact JSON the processor gives for an assembled expanded document,
/// or the rendering of its failure.
pub(crate) async fn processor_document(compactor: &ContextCompactor<'_>, document: &Value) -> Result<Value, String> {
    compactor
        // clone: the processor consumes the document the corpus keeps
        .compact(ExpandedInput::Assembled(document.clone()), CompactOptions::default())
        .await
        .map_err(|error| error.to_string())
}
