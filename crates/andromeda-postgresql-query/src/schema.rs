//! Database schema identifiers for sea-query.
//!
//! Defines `Iden` enums for all tables and columns used in SQL query building.
//! These provide type-safe table and column references that sea-query uses to
//! generate properly quoted SQL identifiers.

use sea_query::Iden;

/// The `entities` table - stores current entity state.
#[derive(Iden, Clone, Copy)]
pub enum Entities {
    /// Table name: `entities`
    Table,
    /// Primary key: entity URN identifier
    Id,
    /// Array of entity type IRIs
    Types,
    /// Full expanded JSON-LD payload
    Payload,
}

/// The `temporal_entity_attributes` table - tracks attribute metadata for temporal queries.
#[derive(Iden, Clone, Copy)]
pub enum TemporalEntityAttributes {
    /// Table name: `temporal_entity_attributes`
    Table,
    /// Primary key
    Id,
    /// Foreign key to entity
    EntityId,
    /// Foreign key to attribute_names
    AttributeNameId,
    /// Optional dataset identifier
    DatasetId,
}

/// The `attribute_history` table - stores historical attribute values.
#[derive(Iden, Clone, Copy)]
pub enum AttributeHistory {
    /// Table name: `attribute_history`
    Table,
    /// Foreign key to temporal_entity_attributes
    TeaId,
}

/// The `attribute_names` table - normalized attribute name storage.
#[derive(Iden, Clone, Copy)]
pub enum AttributeNames {
    /// Table name: `attribute_names`
    Table,
    /// Primary key
    Id,
    /// Expanded attribute IRI
    Name,
}

/// Alias `e` for entities table.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
pub struct E;

/// Alias `tea` for temporal_entity_attributes table.
#[derive(Iden, Clone, Copy)]
#[iden = "tea"]
pub struct Tea;

/// Alias `ah` for attribute_history table.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
pub struct Ah;

/// Alias `an` for attribute_names table.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
pub struct An;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_table_name() {
        assert_eq!(Entities::Table.to_string(), "entities");
    }

    #[test]
    fn entities_column_names() {
        assert_eq!(Entities::Id.to_string(), "id");
        assert_eq!(Entities::Types.to_string(), "types");
        assert_eq!(Entities::Payload.to_string(), "payload");
    }

    #[test]
    fn temporal_entity_attributes_names() {
        assert_eq!(TemporalEntityAttributes::Table.to_string(), "temporal_entity_attributes");
        assert_eq!(TemporalEntityAttributes::EntityId.to_string(), "entity_id");
        assert_eq!(TemporalEntityAttributes::AttributeNameId.to_string(), "attribute_name_id");
    }

    #[test]
    fn attribute_history_names() {
        assert_eq!(AttributeHistory::Table.to_string(), "attribute_history");
        assert_eq!(AttributeHistory::TeaId.to_string(), "tea_id");
    }

    #[test]
    fn table_aliases() {
        assert_eq!(E.to_string(), "e");
        assert_eq!(Tea.to_string(), "tea");
        assert_eq!(Ah.to_string(), "ah");
        assert_eq!(An.to_string(), "an");
    }
}
