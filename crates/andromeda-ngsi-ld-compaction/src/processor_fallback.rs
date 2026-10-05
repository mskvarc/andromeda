//! Compacting through the JSON-LD processor what the term walk leaves to it.
//!
//! The model is written in its canonical expanded form, which is the stored
//! document, and compacted as the broker has always compacted that document;
//! the result is therefore exactly the one the processor gives. Its text is
//! read by the compact model as every compact document is, through the terms
//! the request context defines for core terms.

use crate::{compaction_error::CompactionError, term_context::TermContext};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_jsonld::engine::{context_compactor::ContextCompactor, expanded_input::ExpandedInput, options::CompactOptions};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity::Entity, node_refusal::NodeReadError};
use andromeda_ngsi_ld_representation::{
    compact_node::{CompactNode, NodeShape, aliased_node::AliasedNode},
    sys_attrs::SysAttrsInclusion,
};
use serde_json::{Value, value::RawValue};
use std::{convert::Infallible, sync::Arc};

/// Compact entity state through the processor.
pub(crate) async fn entity_through_processor(
    compactor: &ContextCompactor<'_>,
    terms: &TermContext,
    state: &EntityState,
    id_override: Option<&Identifier>,
    sys_attrs: SysAttrsInclusion,
) -> Result<Entity, CompactionError> {
    let document = match sys_attrs {
        SysAttrsInclusion::Include => serde_json::to_value(state),
        SysAttrsInclusion::Strip => serde_json::to_value(without_instance_times(state)),
    }
    .map_err(CompactionError::Encode)?;
    let compacted = compactor.compact(ExpandedInput::Canonical(document), CompactOptions::default()).await?;
    let text = serde_json::to_string(&compacted).map_err(CompactionError::Encode)?;
    let raw: &RawValue = serde_json::from_str(&text).map_err(CompactionError::Encode)?;
    let mut entity = read_entity(AliasedNode::new(JsonPiece::new(raw), terms.aliases()))?;
    if let Some(id) = id_override {
        // clone: the compact entity owns the identifier it is read by
        entity.id = id.clone();
    }
    Ok(entity)
}

/// The entity a compacted document holds: the document, or the first node of
/// the array the processor answered with.
fn read_entity<'n, N: CompactNode<'n, Unreadable = Infallible>>(document: N) -> Result<Entity, CompactionError> {
    let node = match document.shape() {
        NodeShape::Array(nodes) => nodes.into_iter().next().unwrap_or(document),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => document,
    };
    <Entity>::from_node(node).map_err(|error| match error {
        NodeReadError::Refused(refused) => CompactionError::Refused(refused),
        NodeReadError::Unreadable(never) => match never {},
    })
}

/// Compact a written document through the processor.
pub(crate) async fn document_through_processor(compactor: &ContextCompactor<'_>, document: &EntityDocument) -> Result<Value, CompactionError> {
    let document = serde_json::to_value(document).map_err(CompactionError::Encode)?;
    Ok(compactor.compact(ExpandedInput::Assembled(document), CompactOptions::default()).await?)
}

/// `state` without the clause 4.8 times of its top-level instances, which a
/// representation that did not ask for `sysAttrs` leaves out (Table
/// 5.2.14-1).
fn without_instance_times(state: &EntityState) -> EntityState {
    // clone: cheap, every attribute stays shared until its instances are touched
    let mut stripped = state.clone();
    stripped.attributes_mut().retain(|_, instances| {
        for (_, instance) in Arc::make_mut(instances).iter_mut() {
            instance.set_created_at(None);
            instance.set_modified_at(None);
        }
        true
    });
    stripped
}
