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
                    if let Some(type_name) = entity.get("type").and_then(Value::as_str) {
                        let expanded = self.engine.expand_type(type_name, &prepared_context).await?.to_string();
                        if let Some(object) = entity.as_object_mut() {
                            object.insert("type".to_string(), Value::String(expanded));
                        }
                    }
                }
            }

            self.expand_attribute_array(entry, "propertyNames", &prepared_context).await?;
            self.expand_attribute_array(entry, "relationshipNames", &prepared_context).await?;
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
                    if let Some(type_name) = entity.get("type").and_then(Value::as_str) {
                        let compacted = compact_type_iri(&self.engine, type_name, context).await;
                        if let Some(object) = entity.as_object_mut() {
                            object.insert("type".to_string(), Value::String(compacted));
                        }
                    }
                }
            }

            self.compact_attribute_array(entry, "propertyNames", context).await;
            self.compact_attribute_array(entry, "relationshipNames", context).await;
        }
        Ok(())
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
