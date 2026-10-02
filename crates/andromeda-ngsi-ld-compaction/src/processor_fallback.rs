//! Compacting through the JSON-LD processor what the term walk leaves to it.
//!
//! The model is written in its canonical expanded form, which is the stored
//! document, and compacted as the broker has always compacted that document;
//! the result is therefore exactly the one the processor gives.

use crate::{compacted_entity::decode_entity, compaction_error::CompactionError};
use andromeda_jsonld::engine::{context_compactor::ContextCompactor, expanded_input::ExpandedInput, options::CompactOptions};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde_json::Value;
use std::sync::Arc;

/// Compact entity state through the processor.
pub(crate) async fn entity_through_processor(
    compactor: &ContextCompactor<'_>,
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
    decode_entity(compacted, id_override)
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
