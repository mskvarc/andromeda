//! The two stages of the names a query language carries.
//!
//! A query tree is written in the terms a client chose (a term, a compact IRI
//! or an absolute IRI) and read, once expanded through the request `@context`,
//! by absolute IRIs. The tree has the same shape in both stages, so it is
//! generic over a [`NameStage`] that names the type of its attribute names and
//! of its entity type names, and the compiler keeps a consumer from reading a
//! tree in the stage it was not built for.

use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName, entity_type_iri::EntityTypeIri, entity_type_name::EntityTypeName};
use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

/// The type of the attribute names and of the entity type names a query tree
/// carries in one stage.
pub trait NameStage: Copy + Debug + Default + Eq + Hash + Ord + Send + Sync + 'static {
    /// The name of an attribute in this stage.
    type Attribute: Clone + Debug + Display + Eq + Hash + Ord + AsRef<str> + Send + Sync + 'static;
    /// The name of an entity type in this stage.
    type EntityType: Clone + Debug + Display + Eq + Hash + Ord + AsRef<str> + Send + Sync + 'static;
}

/// The stage of a tree as its client wrote it: names are terms, compact IRIs or
/// absolute IRIs, within the clause 4.6.2 name grammar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Written;

/// The stage of a tree after expansion through the request `@context`: names
/// are absolute IRIs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Expanded;

impl NameStage for Written {
    type Attribute = AttributeName;
    type EntityType = EntityTypeName;
}

impl NameStage for Expanded {
    type Attribute = AttributeIri;
    type EntityType = EntityTypeIri;
}
