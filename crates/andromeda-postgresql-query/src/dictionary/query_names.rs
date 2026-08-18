//! The two dictionaries one query resolves before it can be rendered.

use crate::dictionary::resolved_names::ResolvedNames;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

/// Every dictionary key a compiled query renders against.
///
/// The `q` filter, the geo query and the distance ordering read attributes by
/// key; the type selection and a linked-entity traversal's type guard read
/// entity types by key. Both sets are resolved once, before rendering, and
/// travel together so a fragment that needs either has it.
#[derive(Debug, Clone, Default)]
pub struct QueryNames {
    /// The attribute names the query reads by dictionary key.
    pub attributes: ResolvedNames<AttributeName>,
    /// The entity type names the query reads by dictionary key.
    pub types: ResolvedNames<EntityTypeName>,
}
