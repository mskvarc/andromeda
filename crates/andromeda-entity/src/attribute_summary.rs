//! What the Entities of one tenant or Snapshot carry of one attribute name.
//!
//! Backs the clause 5.7.10 Retrieve Available Attribute Information.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};

/// What the existing Entities carry of one attribute name (clause 5.2.28).
#[derive(Debug, PartialEq, Eq)]
pub struct AttributeSummary {
    /// The expanded IRI of the attribute, named as an attribute reference.
    pub attr_iri: AttributeName,
    /// Table 5.2.28-1 `attributeCount`: the number of attribute instances with
    /// this name. Every `datasetId` instance counts (clause 4.5.5), and an
    /// Entity with several types counts once.
    pub instance_count: u64,
    /// The types of the Entities that carry the attribute, in name order.
    pub entity_types: Vec<EntityTypeName>,
    /// The attribute families its instances declare, in family order.
    pub attribute_types: Vec<AttributeType>,
}
