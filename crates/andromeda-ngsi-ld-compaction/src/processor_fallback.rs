//! Compacting through the JSON-LD processor what the term walk leaves to it.
//!
//! The model is written in its canonical expanded form, which is the stored
//! document, and compacted from that document;
//! the result is therefore exactly the one the processor gives. Its text is
//! read by the compact model as every compact document is, without the
//! containers of the request context's terms and through the terms it defines
//! for core terms.

use crate::{compaction_error::CompactionError, patch_reading::PatchReading, term_context::TermContext};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_jsonld::engine::{context_compactor::ContextCompactor, expanded_text::ExpandedText, options::CompactOptions};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    attribute::member_reading::MemberReading,
    entity::{Entity, EntityPatch},
    node_refusal::NodeReadError,
    temporal::{temporal_attribute::TemporalAttribute, temporal_entity::TemporalEntity},
};
use andromeda_ngsi_ld_representation::{
    compact_node::{CompactNode, NodeShape},
    sys_attrs::SysAttrsInclusion,
};
use std::convert::Infallible;

/// Compact entity state through the processor.
pub(crate) async fn entity_through_processor(
    compactor: &ContextCompactor<'_>,
    terms: &TermContext,
    state: &EntityState,
    id_override: Option<&Identifier>,
    sys_attrs: SysAttrsInclusion,
) -> Result<Entity, CompactionError> {
    let document = match sys_attrs {
        SysAttrsInclusion::Include => serde_json::to_vec(state),
        SysAttrsInclusion::Strip => serde_json::to_vec(&without_instance_times(state)),
    }
    .map_err(CompactionError::Encode)?;
    let compacted = compactor.compact(ExpandedText::Canonical(&document), CompactOptions::default()).await?;
    let mut entity = read_entity(terms.read(JsonPiece::new(&compacted)))?;
    if let Some(id) = id_override {
        // clone: the compact entity owns the identifier it is read by
        entity.id = id.clone();
    }
    Ok(entity)
}

/// The entity a compacted document holds: the document, or the first node of
/// the array the processor answered with.
fn read_entity<'n, N: CompactNode<'n, Unreadable = Infallible>>(document: N) -> Result<Entity, CompactionError> {
    <Entity>::from_node(first_node(document)).map_err(|error| match error {
        NodeReadError::Refused(refused) => CompactionError::Refused(refused),
        NodeReadError::Unreadable(never) => match never {},
    })
}

/// The partial entity a compacted document holds, read as [`read_entity`]
/// reads an entity and its attributes as `reading` says.
fn read_patch<'n, M: MemberReading, N: CompactNode<'n, Unreadable = Infallible>>(
    document: N,
    reading: PatchReading,
) -> Result<EntityPatch<M>, CompactionError> {
    reading.read(first_node(document)).map_err(|error| match error {
        NodeReadError::Refused(refused) => CompactionError::Refused(refused),
        NodeReadError::Unreadable(never) => match never {},
    })
}

/// The document itself, or the first node of the array the processor answered
/// with.
fn first_node<'n, N: CompactNode<'n>>(document: N) -> N {
    match document.shape() {
        NodeShape::Array(nodes) => nodes.into_iter().next().unwrap_or(document),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => document,
    }
}

/// Compact a written document through the processor, reading the compacted
/// text as the partial entity a fragment is.
pub(crate) async fn document_patch_through_processor<M: MemberReading>(
    compactor: &ContextCompactor<'_>,
    terms: &TermContext,
    document: &EntityDocument,
    reading: PatchReading,
) -> Result<EntityPatch<M>, CompactionError> {
    let document = serde_json::to_vec(document).map_err(CompactionError::Encode)?;
    let compacted = compactor.compact(ExpandedText::Assembled(&document), CompactOptions::default()).await?;
    read_patch(terms.read(JsonPiece::new(&compacted)), reading)
}

/// Compact the document of a Temporal Evolution through the processor,
/// reading the compacted text as the compact Temporal Evolution it is.
pub(crate) async fn temporal_through_processor(
    compactor: &ContextCompactor<'_>,
    terms: &TermContext,
    document: &EntityDocument,
) -> Result<TemporalEntity, CompactionError> {
    let document = serde_json::to_vec(document).map_err(CompactionError::Encode)?;
    let compacted = compactor.compact(ExpandedText::Assembled(&document), CompactOptions::default()).await?;
    let compacted = terms.read(JsonPiece::new(&compacted));
    let node = match compacted.shape() {
        NodeShape::Array(nodes) => nodes.into_iter().next().unwrap_or(compacted),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => compacted,
    };
    TemporalEntity::from_node(node).map_err(|error| match error {
        NodeReadError::Refused(refused) => CompactionError::Refused(refused),
        NodeReadError::Unreadable(never) => match never {},
    })
}

/// Compact the document of a reified Scope history through the processor,
/// reading the compacted text for its `scope` member.
pub(crate) async fn scope_history_through_processor(
    compactor: &ContextCompactor<'_>,
    terms: &TermContext,
    document: &EntityDocument,
) -> Result<Option<TemporalAttribute>, CompactionError> {
    let document = serde_json::to_vec(document).map_err(CompactionError::Encode)?;
    let compacted = compactor.compact(ExpandedText::Assembled(&document), CompactOptions::default()).await?;
    TemporalAttribute::scope_history_of(first_node(terms.read(JsonPiece::new(&compacted)))).map_err(|error| match error {
        NodeReadError::Refused(refused) => CompactionError::Refused(refused),
        NodeReadError::Unreadable(never) => match never {},
    })
}

/// Compact a written document through the processor, reading the compacted
/// text as the compact entity it is.
pub(crate) async fn document_entity_through_processor(
    compactor: &ContextCompactor<'_>,
    terms: &TermContext,
    document: &EntityDocument,
) -> Result<Entity, CompactionError> {
    let document = serde_json::to_vec(document).map_err(CompactionError::Encode)?;
    let compacted = compactor.compact(ExpandedText::Assembled(&document), CompactOptions::default()).await?;
    read_entity(terms.read(JsonPiece::new(&compacted)))
}

/// `state` without the clause 4.8 times of its instances at every depth,
/// which a representation that did not ask for `sysAttrs` leaves out (Table
/// 5.2.14-1).
fn without_instance_times(state: &EntityState) -> EntityState {
    // clone: cheap, every attribute stays shared until its instances are touched
    let mut stripped = state.clone();
    stripped.strip_attribute_instants();
    stripped
}
