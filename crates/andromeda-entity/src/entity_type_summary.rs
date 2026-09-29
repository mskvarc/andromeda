//! What the Entities carrying one type hold.
//!
//! Backs the clause 5.7.7 Retrieve Available Entity Type Information.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};

/// What the existing Entities carrying one type hold (clause 5.2.26).
#[derive(Debug, PartialEq, Eq)]
pub struct EntityTypeSummary {
    /// Table 5.2.26-1 `entityCount`: the number of Entities carrying the type,
    /// whatever other types they carry.
    pub entity_count: u64,
    /// Every attribute the Entities of the type carry, in name order.
    pub attributes: Vec<AttributeFamilies>,
}

/// One attribute the Entities of a type carry, with the families its
/// instances on those Entities declare.
#[derive(Debug, PartialEq, Eq)]
pub struct AttributeFamilies {
    /// The expanded attribute name.
    pub name: AttributeName,
    /// The attribute families of its instances on Entities of the type, in
    /// family order: clause 5.7.7 reports the types the attribute has among
    /// the Entities of the type asked about, not among every Entity.
    pub families: Vec<AttributeType>,
}
