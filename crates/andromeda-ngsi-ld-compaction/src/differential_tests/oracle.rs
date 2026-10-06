//! The processor's compaction, as the read path has always performed it.
//!
//! This is the reading the walk is held to: the stored document with its
//! attribute-instance system times stripped when the request did not ask for
//! `sysAttrs`, compacted by the processor and read by the compact model
//! through the terms the context defines for core terms, its expired instances
//! dropped.

use crate::core_aliases::core_term_aliases;
use andromeda_json_literal::piece::JsonPiece;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{context_compactor::ContextCompactor, expanded_text::ExpandedText, options::CompactOptions, processor::JsonLdEngine},
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::IdentifierParseError;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::{
    compact_node::{CompactNode, NodeShape, aliased_node::AliasedNode},
    sys_attrs::SysAttrsInclusion,
};
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
        SysAttrsInclusion::Include => serde_json::to_vec(stored).expect("the stored document renders"),
        SysAttrsInclusion::Strip => {
            let mut state = EntityState::from_slice(&serde_json::to_vec(stored).expect("the stored document renders")).expect("the stored document is a state");
            state.strip_attribute_instants();
            serde_json::to_vec(&state).expect("the stripped state renders")
        }
    };
    let compacted = compactor
        .compact(ExpandedText::Canonical(&expanded), CompactOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    let compacted = JsonPiece::new(&compacted);
    let object = match compacted.shape() {
        NodeShape::Array(items) => items.into_iter().next().unwrap_or(compacted),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => compacted,
    };
    let aliases = core_term_aliases(compactor.processed_context().await.map_err(|error| error.to_string())?.active_context());
    let mut entity = <Entity>::from_node(AliasedNode::new(object, &aliases)).map_err(|error| error.to_string())?;
    if let Some(id) = id_override {
        entity.id = id.parse().map_err(|error: IdentifierParseError| error.to_string())?;
    }
    entity.remove_expired_attributes(now);
    Ok(entity)
}

/// The processor's expansion of `entity` under `spec`, as JSON.
pub(crate) async fn expanded(engine: &JsonLdEngine, entity: &Value, spec: &ContextSpec) -> Value {
    let context = engine.expansion_context(spec).await.expect("the context processes");
    let text = serde_json::to_vec(entity).expect("the entity renders");
    context.expand_text(engine, &text).await.expect("the entity expands").into_serde_json()
}
