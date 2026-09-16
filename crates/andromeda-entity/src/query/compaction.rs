//! Turning an expanded payload back into a compacted [`Entity`].
//!
//! [`Entity`]: andromeda_ngsi_ld_model::entity::Entity

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError, query::EntityQueryService};
use andromeda_jsonld::{context_spec::ContextSpec, engine::options::CompactOptions, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_model::{entity::Entity, expanded::attribute_system_time::strip_attribute_instants};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_query::term_expander::TermExpander;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use iri_rs::IriBuf;
use serde_json::Value;

impl EntityQueryService {
    /// Compact one expanded payload against the active context and parse it as
    /// an [`Entity`].
    ///
    /// `id_override` names the entity when the payload's own `@id` is not the
    /// one the caller asked about, which happens on a batch read keyed by
    /// identifier.
    pub(super) async fn compact_to_entity(
        &self,
        expanded: Value,
        context: &ContextSpec,
        id_override: Option<&str>,
        sys_attrs: SysAttrsInclusion,
    ) -> Result<Entity, EntityError> {
        // Clause 4.8 with Table 5.2.14-1: attribute-instance createdAt/modifiedAt
        // live in the stored payload, so a representation the request did not ask
        // to carry `sysAttrs` has them removed before compaction. The Entity-level
        // pair is injected separately from the store columns only under `Include`.
        let mut expanded = expanded;
        if sys_attrs == SysAttrsInclusion::Strip {
            strip_attribute_instants(&mut expanded);
        }
        let compacted = self.engine().compact(expanded, context, CompactOptions::default()).await?;

        let mut obj = match compacted {
            Value::Array(mut arr) if !arr.is_empty() => match arr.swap_remove(0) {
                Value::Object(map) => Some(map),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
            },
            Value::Object(map) => Some(map),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => None,
        }
        .ok_or_else(|| EntityError::Validation(EntityValidationReason::EntityDataNotObject))?;

        if let Some(t) = obj.remove("@type") {
            obj.insert("type".to_string(), t);
        }

        let id = id_override
            .map(str::to_string)
            .or_else(|| obj.get("id").and_then(Value::as_str).map(str::to_string))
            .or_else(|| obj.get("@id").and_then(Value::as_str).map(str::to_string))
            .ok_or_else(|| EntityError::Validation(EntityValidationReason::EntityMissingId))?;

        obj.remove("@id");
        obj.insert("id".to_string(), Value::String(id));

        let mut entity: Entity = deserialize_exact(&Value::Object(obj)).map_err(|err| EntityError::Validation(EntityValidationReason::EntityData(err)))?;

        // Clause 4.22: an instance whose `expiresAt` has passed is not part of
        // the entity, and the sweep that removes it from storage runs on an
        // interval. Filtering here is what keeps a client from seeing one
        // between sweeps.
        entity.remove_expired_attributes(chrono::Utc::now());

        Ok(entity)
    }

    /// Expand an attribute key of a compacted entity to the IRI it names.
    pub(super) async fn expanded_key(&self, key: &str, context: &ContextSpec) -> Result<IriBuf, EntityError> {
        let prepared_context = self.engine().prepare_term_context(context);
        ContextualExpander::new(self.engine(), &prepared_context)
            .expand_term(key)
            .await
            .map_err(EntityError::from)
    }
}
