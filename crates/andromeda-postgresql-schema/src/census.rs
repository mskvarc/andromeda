//! The discovery census: the ledgers clause 5.7 entity type and attribute
//! discovery is answered from.
//!
//! Each relation is a ledger of signed changes to one count, written by the
//! statements that write `entity_search`; a count is the sum of the rows that
//! share its key.

use sea_query::Iden;

/// The `entity_type_census` table - changes to the number of Entities carrying
/// each type (clause 5.2.26 `entityCount`).
#[derive(Iden, Clone, Copy)]
pub enum EntityTypeCensus {
    /// Table name: `entity_type_census`
    Table,
    /// The `entity_type_names` key of the type counted
    EntityTypeNameKey,
    /// The signed change to the number of Entities carrying the type
    EntityDelta,
}

/// The `entity_type_attribute_census` table - changes to the number of
/// attribute instances of each name and family on the Entities of each type.
#[derive(Iden, Clone, Copy)]
pub enum EntityTypeAttributeCensus {
    /// Table name: `entity_type_attribute_census`
    Table,
    /// The `entity_type_names` key of the type the instances' Entities carry
    EntityTypeNameKey,
    /// The `attribute_names` key of the instances' attribute
    AttributeNameKey,
    /// The attribute family code of the instances
    AttributeKind,
    /// The signed change to the number of such instances
    InstanceDelta,
}

/// The `attribute_census` table - changes to the number of attribute instances
/// of each name across every Entity (Table 5.2.28-1 `attributeCount`).
#[derive(Iden, Clone, Copy)]
pub enum AttributeCensus {
    /// Table name: `attribute_census`
    Table,
    /// The `attribute_names` key of the instances' attribute
    AttributeNameKey,
    /// The signed change to the number of such instances
    InstanceDelta,
}

#[cfg(test)]
mod tests {
    use crate::census::{AttributeCensus, EntityTypeAttributeCensus, EntityTypeCensus};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn entity_type_census_names() {
        assert_eq!(EntityTypeCensus::Table.to_string(), "entity_type_census");
        assert_eq!(EntityTypeCensus::EntityTypeNameKey.to_string(), "entity_type_name_key");
        assert_eq!(EntityTypeCensus::EntityDelta.to_string(), "entity_delta");
    }

    #[test]
    fn entity_type_attribute_census_names() {
        assert_eq!(EntityTypeAttributeCensus::Table.to_string(), "entity_type_attribute_census");
        assert_eq!(EntityTypeAttributeCensus::EntityTypeNameKey.to_string(), "entity_type_name_key");
        assert_eq!(EntityTypeAttributeCensus::AttributeNameKey.to_string(), "attribute_name_key");
        assert_eq!(EntityTypeAttributeCensus::AttributeKind.to_string(), "attribute_kind");
        assert_eq!(EntityTypeAttributeCensus::InstanceDelta.to_string(), "instance_delta");
    }

    #[test]
    fn attribute_census_names() {
        assert_eq!(AttributeCensus::Table.to_string(), "attribute_census");
        assert_eq!(AttributeCensus::AttributeNameKey.to_string(), "attribute_name_key");
        assert_eq!(AttributeCensus::InstanceDelta.to_string(), "instance_delta");
    }
}
