//! The short names a discovery answer renders its stored names as.
//!
//! Tables 5.2.25-1, 5.2.26-1 and 5.2.28-1 all word the rule the same way, a
//! name is rendered as the "short name if contained in @context", so every
//! discovery surface takes the same compaction, and none of them may reach for
//! a compaction of its own. The identifiers beside those names are the
//! opposite: Table 5.2.28-1 fixes the Attribute `id` to the "Full URI of
//! attribute name" and Tables 5.2.25-1 and 5.2.26-1 fix the entity type `id`
//! to the "Fully Qualified Name (FQN) of the entity type being described", so
//! a discovery resource is serialized as it is built rather than compacted as
//! a JSON-LD document, which would shorten the identifiers too.

use crate::{
    discovery::stored_names::{parse_attribute_name, parse_type_name},
    error::EntityError,
};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

/// The short name the context gives a stored attribute name, or the name
/// itself when the context declares no term for it.
pub(super) fn compacted_attribute_name(names: &NameCompaction, iri: &AttributeName) -> Result<AttributeName, EntityError> {
    parse_attribute_name(&names.compact_attr_iri(iri.as_str()))
}

/// The short name the context gives a stored entity type name, or the name
/// itself when the context declares no term for it.
pub(super) fn compacted_type_name(names: &NameCompaction, iri: &EntityTypeName) -> Result<EntityTypeName, EntityError> {
    parse_type_name(&names.compact_type_iri(iri.as_str()))
}

/// The short names the context gives a list of stored attribute names.
pub(super) fn compacted_attribute_names(names: &NameCompaction, iris: impl IntoIterator<Item = AttributeName>) -> Result<Vec<AttributeName>, EntityError> {
    iris.into_iter().map(|iri| compacted_attribute_name(names, &iri)).collect()
}

/// The short names the context gives a list of stored entity type names.
pub(super) fn compacted_type_names(names: &NameCompaction, iris: impl IntoIterator<Item = EntityTypeName>) -> Result<Vec<EntityTypeName>, EntityError> {
    iris.into_iter().map(|iri| compacted_type_name(names, &iri)).collect()
}
