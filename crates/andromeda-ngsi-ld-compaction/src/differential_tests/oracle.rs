//! The processor's compaction, as the read path has always performed it.
//!
//! This is the reading the walk is held to: the stored document with its
//! attribute-instance system times stripped when the request did not ask for
//! `sysAttrs`, compacted by the processor and read by the compact model
//! through the terms the context defines for core terms, its expired instances
//! dropped.

use crate::core_aliases::core_term_aliases;
use andromeda_jsonld::engine::{context_compactor::ContextCompactor, expanded_input::ExpandedInput, options::CompactOptions};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::IdentifierParseError;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::{compact_node::aliased_node::AliasedNode, sys_attrs::SysAttrsInclusion};
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
    let object = match &compacted {
        Value::Array(items) => items.first().unwrap_or(&compacted),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => &compacted,
    };
    let aliases = core_term_aliases(compactor.processed_context().await.map_err(|error| error.to_string())?.active_context());
    let mut entity = <Entity>::from_node(AliasedNode::new(object, &aliases)).map_err(|error| error.to_string())?;
    if let Some(id) = id_override {
        entity.id = id.parse().map_err(|error: IdentifierParseError| error.to_string())?;
    }
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
