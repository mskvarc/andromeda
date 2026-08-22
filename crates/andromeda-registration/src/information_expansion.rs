//! JSON-LD expansion and compaction of registration information entries.

use crate::{RegistrationService, error::RegistrationError};
use andromeda_jsonld::{
    compaction::{compact_attr_iri, compact_type_iri},
    context_spec::ContextSpec,
    engine::term_lookup::PreparedTermContext,
};
use serde_json::Value;

impl RegistrationService {
    /// Expand entity types and attribute names in an `information` array.
    pub(super) async fn expand_information(&self, information: &mut Value, context: &ContextSpec) -> Result<(), RegistrationError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let Some(entries) = information.as_array_mut() else {
            return Ok(());
        };

        for entry in entries {
            if let Some(entities) = entry.get_mut("entities").and_then(Value::as_array_mut) {
                for entity in entities {
                    self.expand_entity_type(entity, &prepared_context).await?;
                }
            }

            self.expand_attribute_array(entry, "propertyNames", &prepared_context).await?;
            self.expand_attribute_array(entry, "relationshipNames", &prepared_context).await?;
        }
        Ok(())
    }

    /// Expand an `EntityInfo` `type`, which Table 5.2.8-1 lets be one name or
    /// an array of names; the shape is kept so it reads back as written.
    async fn expand_entity_type(&self, entity: &mut Value, context: &PreparedTermContext) -> Result<(), RegistrationError> {
        let expanded = match entity.get("type") {
            Some(Value::String(type_name)) => Value::String(self.engine.expand_type(type_name, context).await?.to_string()),
            Some(Value::Array(type_names)) => {
                let mut expanded = Vec::with_capacity(type_names.len());
                for type_name in type_names {
                    match type_name {
                        Value::String(type_name) => expanded.push(Value::String(self.engine.expand_type(type_name, context).await?.to_string())),
                        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => {
                            expanded.push(other.clone()); // clone: a non-string member is kept for the typed deserialisation to reject
                        }
                    }
                }
                Value::Array(expanded)
            }
            Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) | None => return Ok(()),
        };
        if let Some(object) = entity.as_object_mut() {
            object.insert("type".to_string(), expanded);
        }
        Ok(())
    }

    async fn expand_attribute_array(&self, information: &mut Value, member: &str, context: &PreparedTermContext) -> Result<(), RegistrationError> {
        let Some(names) = information.get(member).and_then(Value::as_array) else {
            return Ok(());
        };
        let mut expanded = Vec::with_capacity(names.len());
        for name in names {
            if let Some(raw) = name.as_str() {
                expanded.push(Value::String(self.engine.expand_term(raw, context).await?.to_string()));
            }
        }
        if let Some(object) = information.as_object_mut() {
            object.insert(member.to_string(), Value::Array(expanded));
        }
        Ok(())
    }

    /// Compact entity types and attribute names in an `information` array.
    pub(super) async fn compact_information(&self, information: &mut Value, context: &ContextSpec) -> Result<(), RegistrationError> {
        let Some(entries) = information.as_array_mut() else {
            return Ok(());
        };

        for entry in entries {
            if let Some(entities) = entry.get_mut("entities").and_then(Value::as_array_mut) {
                for entity in entities {
                    self.compact_entity_type(entity, context).await;
                }
            }

            self.compact_attribute_array(entry, "propertyNames", context).await;
            self.compact_attribute_array(entry, "relationshipNames", context).await;
        }
        Ok(())
    }

    /// Compact an `EntityInfo` `type` in whichever of its two shapes it was
    /// stored (Table 5.2.8-1), keeping that shape.
    async fn compact_entity_type(&self, entity: &mut Value, context: &ContextSpec) {
        let compacted = match entity.get("type") {
            Some(Value::String(type_name)) => Value::String(compact_type_iri(&self.engine, type_name, context).await),
            Some(Value::Array(type_names)) => {
                let mut compacted = Vec::with_capacity(type_names.len());
                for type_name in type_names {
                    match type_name {
                        Value::String(type_name) => compacted.push(Value::String(compact_type_iri(&self.engine, type_name, context).await)),
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

    async fn compact_attribute_array(&self, information: &mut Value, member: &str, context: &ContextSpec) {
        let Some(names) = information.get(member).and_then(Value::as_array) else {
            return;
        };
        let mut compacted = Vec::with_capacity(names.len());
        for name in names {
            if let Some(raw) = name.as_str() {
                compacted.push(Value::String(compact_attr_iri(&self.engine, raw, context).await));
            }
        }
        if let Some(object) = information.as_object_mut() {
            object.insert(member.to_string(), Value::Array(compacted));
        }
    }
}
