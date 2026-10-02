//! Walking entity state into the compact [`Entity`].
//!
//! The walk writes the compacted entity with every geometry the compact model
//! reads as typed kept typed, and the compact model reads the tree as it reads
//! any compact tree ([`Entity::from_node`]), so the entity is built without a
//! JSON document of it ever being written. A tree the model's reading leaves
//! to the document is written in full and decoded as the processor's
//! compaction of it would be: an entity the model refuses (the decoding of the
//! document names the refusal), a member carried verbatim around a typed
//! geometry, identity members under names other than `id` and `type`, a tree
//! nested deeper than the reader of a document accepts, and a member name that
//! reader reinterprets.

use crate::{
    compacted::{Compacted, CompactedObject},
    compacted_entity::decode_entity,
    compaction_error::CompactionError,
    term_context::TermContext,
    undecidable::Undecidable,
    walk::{GeometryOutput, Walk},
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use jsonld::syntax::Keyword;
use serde_json::Value;
use thiserror::Error;

/// The deepest nesting of a compacted entity read without its document.
///
/// Well below the nesting `serde_json`'s reader refuses, so every entity read
/// this way is one its document would have been read as; a deeper entity is
/// decoded from its document, which refuses it exactly as before.
const TYPED_READING_DEPTH: usize = 96;

/// The nesting a typed geometry stands for: its object and up to four levels
/// of coordinate arrays.
const GEOMETRY_DEPTH: usize = 5;

/// The prefix of the member names `serde_json`'s reader reinterprets as its
/// own number and raw-value tokens.
const READER_TOKEN_PREFIX: &str = "$serde_json::private::";

/// The member the compact model reads an entity's identifier from.
const ID_MEMBER: &str = "id";

/// The member the compact model reads an entity's types from.
const TYPE_MEMBER: &str = "type";

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
    if reads_without_document(terms, &tree)
        && let Ok(mut entity) = Entity::from_node(&tree)
    {
        if let Some(id) = id_override {
            // clone: the compact entity owns the identifier it is read by
            entity.id = id.clone();
        }
        return Ok(entity);
    }
    let document = match tree.into_written_json() {
        Ok(document) => document,
        Err(_unwritten) => written_document(terms, state, sys_attrs)?,
    };
    decode_entity(document, id_override).map_err(WalkError::Refused)
}

/// The document of `state` with every geometry written out.
fn written_document(terms: &TermContext, state: &EntityState, sys_attrs: SysAttrsInclusion) -> Result<Value, Undecidable> {
    let node = Walk::new(terms, GeometryOutput::Written).compact_state(state, sys_attrs)?;
    node.into_written_json()
        .map(Value::Object)
        .map_err(|_unwritten| Undecidable::UnrepresentableValue)
}

/// Whether the compact model reads `tree` as its document would be read.
pub(crate) fn reads_without_document(terms: &TermContext, tree: &Compacted<'_>) -> bool {
    terms.keyword(Keyword::Id) == ID_MEMBER && terms.keyword(Keyword::Type) == TYPE_MEMBER && plainly_readable(tree, 1)
}

/// Whether `node`, found `depth` containers deep, nests no deeper than
/// [`TYPED_READING_DEPTH`] and names no member the reader reinterprets.
fn plainly_readable(node: &Compacted<'_>, depth: usize) -> bool {
    match node {
        Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::String(_) => true,
        Compacted::Geometry(_) => depth + GEOMETRY_DEPTH <= TYPED_READING_DEPTH,
        Compacted::Array(items) => depth <= TYPED_READING_DEPTH && items.iter().all(|item| plainly_readable(item, depth + 1)),
        Compacted::Object(members) => depth <= TYPED_READING_DEPTH && plainly_readable_members(members, depth),
    }
}

/// Whether every member of an object found `depth` containers deep is
/// plainly readable.
fn plainly_readable_members(members: &CompactedObject<'_>, depth: usize) -> bool {
    members
        .iter()
        .all(|(name, member)| !name.starts_with(READER_TOKEN_PREFIX) && plainly_readable(member, depth + 1))
}
