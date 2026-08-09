//! Aggregated metadata for a single attribute across all entities.
//!
//! Backs the clause 5.7.3 attribute discovery endpoints.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};

/// Aggregated metadata for a single attribute across all entities.
///
/// Backs the clause 5.7.3 attribute discovery endpoints.
#[derive(Debug)]
pub struct AttributeSummary {
    /// The expanded IRI of the attribute, named as an attribute reference.
    pub attr_iri: AttributeName,
    /// Number of distinct entities that carry this attribute.
    pub entity_count: i64,
    /// Entity type IRIs that contain this attribute.
    pub entity_types: Vec<EntityTypeName>,
    /// NGSI-LD attribute types observed for this attribute.
    pub attribute_types: Vec<AttributeType>,
}
