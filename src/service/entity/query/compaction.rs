//! Turning an expanded payload back into a compacted [`Entity`].
//!
//! [`Entity`]: andromeda_ngsi_ld_model::entity::Entity

use crate::{core::error::AndromedaError, service::entity::query::EntityQueryService};
use andromeda_jsonld::{context_spec::ContextSpec, engine::options::CompactOptions, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::term_expander::TermExpander;
use iri_rs::IriBuf;
use serde_json::Value;

impl EntityQueryService {
    /// Compact one expanded payload against the active context and parse it as
    /// an [`Entity`].
    ///
    /// `id_override` names the entity when the payload's own `@id` is not the
    /// one the caller asked about, which happens on a batch read keyed by
    /// identifier.
    pub(super) async fn compact_to_entity(&self, expanded: Value, context: &ContextSpec, id_override: Option<&str>) -> Result<Entity, AndromedaError> {
        let compacted = self.engine().compact(expanded, context, CompactOptions::default()).await?;

        let mut obj = match compacted {
            Value::Array(mut arr) if !arr.is_empty() => match arr.swap_remove(0) {
                Value::Object(map) => Some(map),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
            },
            Value::Object(map) => Some(map),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
        }
        .ok_or_else(|| AndromedaError::entity_validation("Entity data is not a JSON object"))?;

        if let Some(t) = obj.remove("@type") {
            obj.insert("type".to_string(), t);
        }

        let id = id_override
            .map(str::to_string)
            .or_else(|| obj.get("id").and_then(Value::as_str).map(str::to_string))
            .or_else(|| obj.get("@id").and_then(Value::as_str).map(str::to_string))
            .ok_or_else(|| AndromedaError::entity_validation("Entity missing id field"))?;

        obj.remove("@id");
        obj.insert("id".to_string(), Value::String(id));

        let mut entity: Entity =
            serde_json::from_value(Value::Object(obj)).map_err(|err| AndromedaError::entity_validation(format!("Invalid entity data: {err}")))?;

        // Clause 4.22: an instance whose `expiresAt` has passed is not part of
        // the entity, and the sweep that removes it from storage runs on an
        // interval. Filtering here is what keeps a client from seeing one
        // between sweeps.
        entity.remove_expired_attributes(chrono::Utc::now());

        Ok(entity)
    }

    /// Expand an attribute key of a compacted entity to the IRI it names.
    pub(super) async fn expanded_key(&self, key: &str, context: &ContextSpec) -> Result<IriBuf, AndromedaError> {
        ContextualExpander::new(self.engine(), context)
            .expand_term(key)
            .await
            .map_err(AndromedaError::from)
    }
}
