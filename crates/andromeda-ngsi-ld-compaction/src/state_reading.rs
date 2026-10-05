//! Walking entity state into the compact [`Entity`].
//!
//! The walk writes the compacted entity with every geometry the compact model
//! reads as typed kept typed, and the compact model reads the tree as it reads
//! any compact document ([`Entity::from_node`]), each term the request context
//! defines for a core term read as that term. No JSON document of the entity
//! is written on the way.

use crate::{
    compacted::Compacted,
    compaction_error::CompactionError,
    term_context::TermContext,
    undecidable::Undecidable,
    walk::{GeometryOutput, Walk},
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity::Entity, node_refusal::NodeReadError};
use andromeda_ngsi_ld_representation::{compact_node::aliased_node::AliasedNode, sys_attrs::SysAttrsInclusion};
use thiserror::Error;

/// Why the walk gave no compact entity.
#[derive(Debug, Error)]
pub(crate) enum WalkError {
    /// The walk leaves the entity to the processor.
    #[error(transparent)]
    Undecidable(#[from] Undecidable),
    /// The compact model refuses the compacted entity.
    #[error(transparent)]
    Refused(CompactionError),
}

/// Walk `state` into the compact entity.
pub(crate) fn walk_entity(
    terms: &TermContext,
    state: &EntityState,
    id_override: Option<&Identifier>,
    sys_attrs: SysAttrsInclusion,
) -> Result<Entity, WalkError> {
    let tree = Compacted::Object(Walk::new(terms, GeometryOutput::Typed).compact_state(state, sys_attrs)?);
    let mut entity = <Entity>::from_node(AliasedNode::new(&tree, terms.aliases())).map_err(|error| {
        WalkError::Refused(match error {
            NodeReadError::Refused(refused) => CompactionError::Refused(refused),
            NodeReadError::Unreadable(unwritten) => CompactionError::Encode(unwritten),
        })
    })?;
    if let Some(id) = id_override {
        // clone: the compact entity owns the identifier it is read by
        entity.id = id.clone();
    }
    Ok(entity)
}
