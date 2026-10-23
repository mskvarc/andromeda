//! The dictionary keys and the catalogue facts one query resolves before it
//! can be rendered.

use crate::{dictionary::resolved_names::ResolvedNames, history_source::attribute_holders::AttributeHolders};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

/// Every dictionary key a compiled query renders against.
///
/// The `q` filter, the geo query and the distance ordering read attributes by
/// key; the type selection and a linked-entity traversal's type guard read
/// entity types by key. Both sets are resolved once, before rendering, and
/// travel together so a fragment that needs either has it. A temporal
/// selection by attribute alone also reads the types holding each attribute,
/// resolved the same way.
#[derive(Debug, Clone, Default)]
pub struct QueryNames {
    /// The attribute names the query reads by dictionary key.
    pub attributes: ResolvedNames<AttributeName>,
    /// The entity type names the query reads by dictionary key.
    pub types: ResolvedNames<EntityTypeName>,
    /// The types holding streams of each attribute a temporal selection by
    /// attribute alone pages the holders of; empty for every other query.
    pub holders: AttributeHolders,
}
